#!/usr/bin/env python3
"""ui2api: turn a ComfyUI workflow saved by the editor (UI format, with subgraphs) into the API format that POST /prompt takes.

    ui2api.py TEMPLATE.json OBJECT_INFO.json [-o OUT.json]

OBJECT_INFO.json is what GET /object_info of the ComfyUI that will run the workflow answers (it says which inputs of a node are widgets, in
which order the editor saved their values, and which carry a "control after generate" value). Only the standard library is used.

What it does, in the order the editor itself does it (graphToPrompt):
  * a node that is a subgraph is replaced by the nodes inside it (ids "<instance>:<inner>"); the inputs of the subgraph become the links or
    the values the instance had; its outputs become the links from the nodes that make them;
  * a muted node (mode 2) is dropped, a bypassed one (mode 4) passes its input of the same type through, a Reroute passes its input through;
    notes are dropped;
  * the widget values of a node are read in the order of its definition (a dynamic combo takes the values of the option chosen, nested), the
    value that follows a seed (control after generate) is skipped, a widget that is linked takes the link.
Anything it cannot do it says on stderr and leaves out: ComfyUI's own check of the result (POST /prompt) is the test.
"""
from __future__ import annotations

import argparse
import json
import sys

WIDGET_TYPES = {"INT", "FLOAT", "STRING", "BOOLEAN", "COMBO"}
NOTE_TYPES = {"MarkdownNote", "Note"}


def warn(msg: str) -> None:
    print("ui2api: " + msg, file=sys.stderr)


class Defs:
    """What /object_info says about the widgets of a node class."""

    def __init__(self, object_info: dict):
        self.oi = object_info

    def has(self, class_type: str) -> bool:
        return class_type in self.oi

    def sections(self, class_type: str):
        info = self.oi[class_type]
        order = info.get("input_order") or {}
        for sec in ("required", "optional"):
            defs = info["input"].get(sec) or {}
            for name in (order.get(sec) or list(defs.keys())):
                if name in defs:
                    yield name, defs[name]

    @staticmethod
    def is_widget(spec) -> bool:
        t = spec[0]
        opts = spec[1] if len(spec) > 1 and isinstance(spec[1], dict) else {}
        if opts.get("forceInput"):
            return False
        if isinstance(t, list):
            return True
        if t in ("INT", "FLOAT", "STRING", "BOOLEAN", "COMBO", "COMFY_DYNAMICCOMBO_V3"):
            return True
        return False

    @staticmethod
    def has_control(spec) -> bool:
        opts = spec[1] if len(spec) > 1 and isinstance(spec[1], dict) else {}
        return bool(opts.get("control_after_generate"))

    def widget_values(self, class_type: str, values: list):
        """Yield (api_input_name, value) for the widget values of a node saved by the editor."""
        values = list(values or [])
        pos = [0]

        def take():
            if pos[0] >= len(values):
                return None, False
            v = values[pos[0]]
            pos[0] += 1
            return v, True

        def read(name, spec, prefix):
            t = spec[0]
            opts = spec[1] if len(spec) > 1 and isinstance(spec[1], dict) else {}
            if t == "COMFY_DYNAMICCOMBO_V3":
                key, ok = take()
                if not ok:
                    return
                yield prefix + name, key
                for option in opts.get("options", []):
                    if option.get("key") == key:
                        inputs = option.get("inputs") or {}
                        for sec in ("required", "optional"):
                            for nname, nspec in (inputs.get(sec) or {}).items():
                                if self.is_widget(nspec):
                                    yield from read(nname, nspec, prefix + name + ".")
                        break
                return
            v, ok = take()
            if not ok:
                return
            yield prefix + name, v
            if self.has_control(spec):
                take()

        for name, spec in self.sections(class_type):
            if self.is_widget(spec):
                yield from read(name, spec, "")


class Flattener:
    def __init__(self, ui: dict, defs: Defs):
        self.ui = ui
        self.defs = defs
        self.subs = {s["id"]: s for s in ui.get("definitions", {}).get("subgraphs", [])}
        self.out: dict[str, dict] = {}

    # ---- a graph level ---------------------------------------------------------------------------------------------------------------
    class Ctx:
        def __init__(self, nodes, links, prefix, boundary):
            self.nodes = {n["id"]: n for n in nodes}
            self.prefix = prefix
            self.boundary = boundary          # index of a subgraph input -> ("node", gid, slot) | ("value", v) | None
            self.links = {}
            for l in links:
                if isinstance(l, dict):
                    self.links[l["id"]] = (l["origin_id"], l["origin_slot"], l["target_id"], l["target_slot"], l.get("type"))
                else:
                    self.links[l[0]] = (l[1], l[2], l[3], l[4], l[5])
            self.sub_outputs: dict[tuple, object] = {}
            self.expanded: set = set()

    def run(self) -> dict:
        ctx = self.Ctx(self.ui["nodes"], self.ui["links"], "", {})
        self.expand_level(ctx)
        return self.out

    def expand_level(self, ctx: "Flattener.Ctx") -> None:
        for nid in list(ctx.nodes):
            self.emit(ctx, nid)

    def emit(self, ctx: "Flattener.Ctx", nid) -> None:
        node = ctx.nodes[nid]
        t = node["type"]
        if t in NOTE_TYPES or t in ("Reroute", "PrimitiveNode"):
            return
        if node.get("mode", 0) in (2, 4):
            return
        if t in self.subs:
            self.expand_instance(ctx, node)
            return
        if not self.defs.has(t):
            warn(f"unknown node class {t!r} (id {ctx.prefix}{nid}) left out")
            return
        gid = f"{ctx.prefix}{nid}"
        if gid in self.out:
            return
        inputs: dict = {}
        linked: dict = {}
        for ent in node.get("inputs", []):
            if ent.get("link") is not None and ent["link"] in ctx.links:
                o_id, o_slot = ctx.links[ent["link"]][0], ctx.links[ent["link"]][1]
                src = self.resolve(ctx, o_id, o_slot)
                if src is not None:
                    linked[ent["name"]] = src
        for name, value in self.defs.widget_values(t, node.get("widgets_values")):
            if name in linked:
                continue
            inputs[name] = value
        for name, src in linked.items():
            inputs[name] = [src[1], src[2]] if src[0] == "node" else src[1]
        meta = {"title": node.get("title") or t}
        self.out[gid] = {"class_type": t, "inputs": inputs, "_meta": meta}

    # ---- what a link comes from -----------------------------------------------------------------------------------------------------------
    def resolve(self, ctx: "Flattener.Ctx", origin_id, origin_slot):
        if origin_id == -10:
            return ctx.boundary.get(origin_slot)
        node = ctx.nodes.get(origin_id)
        if node is None:
            return None
        t = node["type"]
        mode = node.get("mode", 0)
        if mode == 2:
            return None
        if t in self.subs:
            self.expand_instance(ctx, node)
            return ctx.sub_outputs.get((origin_id, origin_slot))
        if mode == 4 or t == "Reroute":
            outs = node.get("outputs", [])
            want = outs[origin_slot]["type"] if origin_slot < len(outs) else None
            for ent in node.get("inputs", []):
                if ent.get("link") is not None and (want in (None, "*") or ent.get("type") == want or t == "Reroute"):
                    l = ctx.links[ent["link"]]
                    return self.resolve(ctx, l[0], l[1])
            return None
        if t == "PrimitiveNode":
            wv = node.get("widgets_values") or [None]
            return ("value", wv[0])
        self.emit(ctx, origin_id)
        gid = f"{ctx.prefix}{origin_id}"
        if gid not in self.out:
            return None
        return ("node", gid, origin_slot)

    # ---- a subgraph instance --------------------------------------------------------------------------------------------------------------
    def expand_instance(self, ctx: "Flattener.Ctx", inst: dict) -> None:
        if inst["id"] in ctx.expanded:
            return
        ctx.expanded.add(inst["id"])
        sub = self.subs[inst["type"]]
        # the values the editor saved for the widgets of the instance: one for each subgraph input that is not a link-only type, in order
        # (a type such as "IMAGE,MASK" is a link; only these five are widgets)
        widget_inputs = [i for i in sub["inputs"] if i["type"] in WIDGET_TYPES]
        values = list(inst.get("widgets_values") or [])
        value_of = {}
        for k, i in enumerate(widget_inputs):
            if k < len(values):
                value_of[i["name"]] = values[k]
        by_name = {e["name"]: e for e in inst.get("inputs", [])}
        boundary = {}
        for idx, i in enumerate(sub["inputs"]):
            ent = by_name.get(i["name"])
            if ent is not None and ent.get("link") is not None and ent["link"] in ctx.links:
                l = ctx.links[ent["link"]]
                boundary[idx] = self.resolve(ctx, l[0], l[1])
            elif i["name"] in value_of:
                boundary[idx] = ("value", value_of[i["name"]])
            else:
                boundary[idx] = None
        inner = self.Ctx(sub["nodes"], sub["links"], f"{ctx.prefix}{inst['id']}:", boundary)
        self.expand_level(inner)
        for l in sub["links"]:
            lid = l["id"] if isinstance(l, dict) else l[0]
            o_id, o_slot, t_id, t_slot = inner.links[lid][:4]
            if t_id == -20:
                ctx.sub_outputs[(inst["id"], t_slot)] = self.resolve(inner, o_id, o_slot)


def convert(ui: dict, object_info: dict) -> dict:
    return Flattener(ui, Defs(object_info)).run()


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("template")
    ap.add_argument("object_info")
    ap.add_argument("-o", "--out")
    a = ap.parse_args()
    api = convert(json.load(open(a.template)), json.load(open(a.object_info)))
    text = json.dumps(api, indent=1, ensure_ascii=False)
    if a.out:
        open(a.out, "w").write(text + "\n")
    else:
        print(text)
    return 0


if __name__ == "__main__":
    sys.exit(main())

#!/usr/bin/env python3
"""wf_tools: small operations on API-format ComfyUI workflows (a dict of id -> {class_type, inputs, _meta}).

A link is [node_id, output_slot]; anything else in `inputs` is a value. Only the standard library is used.
"""
from __future__ import annotations

import ast
import copy
import math
import operator

PRIMITIVES = {"PrimitiveInt", "PrimitiveFloat", "PrimitiveBoolean", "PrimitiveString", "PrimitiveStringMultiline"}


def is_link(v) -> bool:
    return isinstance(v, list) and len(v) == 2 and isinstance(v[0], str) and isinstance(v[1], int)


def natural_key(k: str):
    return [(0, int(p)) if p.isdigit() else (1, p) for p in k.split(":")]


def replace_refs(g: dict, old_id: str, slot: int, new) -> int:
    n = 0
    for node in g.values():
        for name, v in list(node["inputs"].items()):
            if is_link(v) and v[0] == old_id and v[1] == slot:
                node["inputs"][name] = copy.deepcopy(new)
                n += 1
    return n


def consumers(g: dict, nid: str):
    for cid, node in g.items():
        for name, v in node["inputs"].items():
            if is_link(v) and v[0] == nid:
                yield cid, name, v[1]


def drop_input(g: dict, nid: str, name: str) -> None:
    g[nid]["inputs"].pop(name, None)


def inline_primitives(g: dict, keep=()) -> None:
    """Replace every Primitive* node that holds a literal by its value in the inputs that read it (except those named in keep)."""
    changed = True
    while changed:
        changed = False
        for nid in list(g):
            node = g[nid]
            if node["class_type"] in PRIMITIVES and nid not in keep and "value" in node["inputs"] and not is_link(node["inputs"]["value"]):
                replace_refs(g, nid, 0, node["inputs"]["value"])
                del g[nid]
                changed = True


def fold_switches(g: dict) -> None:
    """A ComfySwitchNode whose switch is a literal is replaced by the branch it chooses."""
    changed = True
    while changed:
        changed = False
        for nid in list(g):
            node = g[nid]
            if node["class_type"] == "ComfySwitchNode" and isinstance(node["inputs"].get("switch"), bool):
                chosen = node["inputs"].get("on_true" if node["inputs"]["switch"] else "on_false")
                if chosen is None:
                    raise ValueError(f"switch {nid} chooses an input it does not have")
                replace_refs(g, nid, 0, chosen)
                del g[nid]
                changed = True


_OPS = {ast.Add: operator.add, ast.Sub: operator.sub, ast.Mult: operator.mul, ast.Div: operator.truediv, ast.FloorDiv: operator.floordiv,
        ast.Mod: operator.mod, ast.Pow: operator.pow}
_FUNCS = {"max": max, "min": min, "round": round, "abs": abs, "floor": math.floor, "ceil": math.ceil, "sqrt": math.sqrt, "int": int}


def safe_eval(expr: str, names: dict):
    """The arithmetic a ComfyMathExpression node does (numbers, names, + - * / // % **, a few functions), and nothing else."""
    def ev(n):
        if isinstance(n, ast.Expression):
            return ev(n.body)
        if isinstance(n, ast.Constant) and isinstance(n.value, (int, float)):
            return n.value
        if isinstance(n, ast.Name) and n.id in names:
            return names[n.id]
        if isinstance(n, ast.BinOp) and type(n.op) in _OPS:
            return _OPS[type(n.op)](ev(n.left), ev(n.right))
        if isinstance(n, ast.UnaryOp) and isinstance(n.op, (ast.USub, ast.UAdd)):
            return -ev(n.operand) if isinstance(n.op, ast.USub) else ev(n.operand)
        if isinstance(n, ast.Call) and isinstance(n.func, ast.Name) and n.func.id in _FUNCS and not n.keywords:
            return _FUNCS[n.func.id](*[ev(a) for a in n.args])
        raise ValueError("not arithmetic: " + ast.dump(n)[:80])
    return ev(ast.parse(expr.strip(), mode="eval"))


def fold_math(g: dict, keep=()) -> None:
    """A ComfyMathExpression whose inputs are all literals is replaced by its three outputs (float, int, bool)."""
    changed = True
    while changed:
        changed = False
        for nid in list(g):
            node = g[nid]
            if node["class_type"] != "ComfyMathExpression" or nid in keep:
                continue
            vals = {k.split(".", 1)[1]: v for k, v in node["inputs"].items() if k.startswith("values.")}
            if any(is_link(v) for v in vals.values()):
                continue
            result = safe_eval(node["inputs"]["expression"], vals)
            replace_refs(g, nid, 0, float(result))
            replace_refs(g, nid, 1, int(result))
            replace_refs(g, nid, 2, bool(result))
            del g[nid]
            changed = True


def prune(g: dict, roots) -> None:
    """Keep the nodes the roots (output nodes) read, directly or not, and drop the rest."""
    seen = set()
    stack = list(roots)
    while stack:
        nid = stack.pop()
        if nid in seen or nid not in g:
            continue
        seen.add(nid)
        for v in g[nid]["inputs"].values():
            if is_link(v):
                stack.append(v[0])
    for nid in list(g):
        if nid not in seen:
            del g[nid]


def simplify(g: dict, roots, keep=()) -> None:
    for _ in range(3):
        inline_primitives(g, keep)
        fold_switches(g)
        fold_math(g, keep)
    inline_primitives(g, keep)
    prune(g, roots)


def strip_prefix(g: dict) -> dict:
    """Drop the "instance:" prefix of ids made from a subgraph when that makes no two ids equal."""
    new = {k: k.rsplit(":", 1)[-1] for k in g}
    if len(set(new.values())) != len(new):
        return g
    out = {}
    for k in sorted(g, key=natural_key):
        node = copy.deepcopy(g[k])
        for name, v in node["inputs"].items():
            if is_link(v):
                node["inputs"][name] = [new[v[0]], v[1]]
        out[new[k]] = node
    return out


def sort_graph(g: dict) -> dict:
    return {k: g[k] for k in sorted(g, key=natural_key)}


def set_in(g: dict, nid: str, name: str, value) -> None:
    if nid not in g:
        raise KeyError(f"no node {nid}")
    g[nid]["inputs"][name] = value


def add(g: dict, nid: str, class_type: str, title: str | None = None, **inputs) -> None:
    if nid in g:
        raise KeyError(f"node {nid} exists")
    g[nid] = {"class_type": class_type, "inputs": inputs, "_meta": {"title": title or class_type}}


def title(g: dict, nid: str, text: str) -> None:
    g[nid].setdefault("_meta", {})["title"] = text


def find(g: dict, class_type: str):
    return [k for k, v in g.items() if v["class_type"] == class_type]


def one(g: dict, class_type: str, **where) -> str:
    """The id of the one node of that class whose inputs equal `where` (a value, or a link written as a list)."""
    hits = [k for k in find(g, class_type) if all(g[k]["inputs"].get(a) == b for a, b in where.items())]
    if len(hits) != 1:
        raise LookupError(f"{class_type} {where}: {len(hits)} found: {hits}")
    return hits[0]

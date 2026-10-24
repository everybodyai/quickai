"""QuickAI's button for ComfyUI-Manager, in ComfyUI's page.

QuickAI writes this folder beside ComfyUI's models folder every time it starts ComfyUI, and tells ComfyUI to read it as a
folder of custom nodes (like quickai_bridge, which has nothing to do with it). It has no nodes, no route and no setting: only
a script for the page (web/quickai_manager.js), which puts a "Manager" button at the top of the page when ComfyUI-Manager is
there, and does nothing when it is not.

Why: the classic Manager, which QuickAI starts for a build made with it (it is the one that has the Model Manager, and the one
whose work is all done by the server, so that the page's --offline does not stop it), puts its buttons in the page's old menu,
which the page hides unless the person has set "Use new menu" to "Disabled" in its settings. Without this, the person who ticked
the box would have nowhere to click.
"""

NODE_CLASS_MAPPINGS = {}
NODE_DISPLAY_NAME_MAPPINGS = {}
WEB_DIRECTORY = "./web"

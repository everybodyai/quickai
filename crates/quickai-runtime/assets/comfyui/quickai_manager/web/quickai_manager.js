// QuickAI's button for ComfyUI-Manager, the page's half (the other is __init__.py, which only says where this is).
//
// The classic Manager shows its menu through a command of its own ("Comfy.Manager.Menu.ToggleVisibility": Custom Nodes Manager,
// Model Manager, Install Missing Custom Nodes, ...), and puts its button in the page's old menu, which the page does not show
// by default. This puts a button at the top of the page, beside the others of its action bar, that runs that command.
// It is made only when ComfyUI answers that the Manager is there: for a ComfyUI without it nothing is added to the page.

import { api } from "../../scripts/api.js";
import { app } from "../../scripts/app.js";

const COMMAND = "Comfy.Manager.Menu.ToggleVisibility";

async function managerIsThere() {
  try {
    const answer = await api.fetchApi("/v2/manager/version");
    return answer.ok;
  } catch (error) {
    return false;
  }
}

if (await managerIsThere()) {
  app.registerExtension({
    name: "QuickAI.ManagerButton",
    actionBarButtons: [
      {
        icon: "mdi mdi-puzzle",
        label: "Manager",
        tooltip: "ComfyUI-Manager: custom nodes and models",
        onClick: () => app.extensionManager.command.execute(COMMAND),
      },
    ],
  });
}

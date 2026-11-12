// QuickAI's button for ComfyUI-Manager, the page's half (the other is __init__.py, which only says where this is).
//
// The classic Manager shows its menu through a command of its own ("Comfy.Manager.Menu.ToggleVisibility": Custom Nodes Manager,
// Model Manager, Install Missing Custom Nodes, ...), and puts its button in the page's old menu, which the page does not show
// by default. This puts a button at the top of the page, beside the others of its action bar, that runs that command.
// It is made only when ComfyUI answers that the Manager is there: for a ComfyUI without it nothing is added to the page.
//
// QuickAI's window opens the page with `?quickai=manager` when the person asks it for the Manager (its "Open the Manager"):
// the page then opens the Manager's menu and, in it, the Model Manager, once, and takes the word out of its address, so that
// a reload does not open them again. A Manager whose menu has no "Model Manager" leaves its menu open.

import { api } from "../../scripts/api.js";
import { app } from "../../scripts/app.js";

const COMMAND = "Comfy.Manager.Menu.ToggleVisibility";
const ASKED = "quickai";
const MANAGER = "manager";

async function managerIsThere() {
  try {
    const answer = await api.fetchApi("/v2/manager/version");
    return answer.ok;
  } catch (error) {
    return false;
  }
}

// Whether the page was opened for the Manager; the word is taken out of the address either way.
function askedForTheManager() {
  const address = new URL(window.location.href);
  const asked = address.searchParams.get(ASKED) === MANAGER;
  if (address.searchParams.has(ASKED)) {
    address.searchParams.delete(ASKED);
    window.history.replaceState(window.history.state, "", address.pathname + address.search + address.hash);
  }
  return asked;
}

// The Model Manager's button of the Manager's menu, once the menu is drawn.
function modelManagerButton() {
  return [...document.querySelectorAll("button")].find((b) => b.textContent.trim() === "Model Manager");
}

async function openTheModelManager() {
  await app.extensionManager.command.execute(COMMAND);
  for (let tries = 0; tries < 20; tries += 1) {
    const button = modelManagerButton();
    if (button) {
      button.click();
      return;
    }
    await new Promise((done) => setTimeout(done, 100));
  }
}

const asked = askedForTheManager();

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
    async setup() {
      if (asked) {
        try {
          await openTheModelManager();
        } catch (error) {
          console.warn("QuickAI: the Manager could not be opened", error);
        }
      }
    },
  });
}

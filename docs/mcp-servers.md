# Giving your model tools with MCP servers

On its own, a model can only produce text. An **MCP server** gives it tools: reading a file, fetching a web page, or
looking something up. (MCP, the Model Context Protocol, is a standard way of offering tools to a model.) A server is
not a model: you keep the model you use and add tools to it.

In QuickAI, MCP servers work:

- in **QuickAI's own chat**, with **llama.cpp** and **Ollama**, and with a model running on **another computer** you
  have added. QuickAI runs the servers itself, on this computer, and asks you before a tool runs, unless you choose
  otherwise;
- in **llama.cpp's chat page** in your browser (**Open Chat UI**), which receives the same servers.

ComfyUI has no chat, so it does not use them. Applications you connect to the model do not receive them either; they
provide their own tools.

## Example: letting a model read a file

This example starts its server with `npx`, so it requires [Node.js](https://nodejs.org/). The model must be one
designed for tool use; Bonsai 2 4B, the model included in QuickAI's full packages, supports this.

1. Place a small text file in a folder, for example `notes.txt` in your Documents folder.
2. Select **MCP servers** in the sidebar.
3. Fill in the form and press **Add**:
   - **Name:** `files`
   - **Command or address:** `npx -y @modelcontextprotocol/server-filesystem ~/Documents`
4. Under **What they offer**, the server shows **Starting…**, then **Ready · 14 tools**. The first time, `npx`
   downloads the server, which can take a minute.
5. Press the back arrow, then **Start**. The wrench icon in the chat box, beside the attachment icon, shows a **1** in
   its corner: the model may use one server.
6. Ask for something that requires the tool, giving the file's full path:

   > Read the file /home/me/Documents/notes.txt and tell me what it says.

7. A card shows the tool the model selected (**files · read_text_file**) and the input it intends to pass. Review it
   and press **Allow once**.
8. The tool runs, the card shows **Used** (**Show** displays the tool's input and output), and the model answers with
   the file's contents.

The first message takes the longest, because the descriptions of this server's 14 tools are sent with it, about 2,500
tokens. On an 8-core CPU this took over a minute; the same request repeated took a few seconds, because the engine had
already processed that part.

## Adding a server

Everything is managed on the **MCP servers** page in the sidebar. There are three ways to add a server, and a server's
own instructions usually provide what you need for one of them.

### By command

Enter the command on a single line, with quotes around any path that contains spaces:

```text
npx -y @modelcontextprotocol/server-filesystem "/home/me/My Notes"
```

If the server requires a key, enter it under **Variables**, one per line: `API_KEY=...`

The command is not run in a shell, so `~` and `$HOME` reach the server exactly as typed. The filesystem server
understands `~`; if another server does not, use the full path.

### By address

Some servers run on the internet. Enter the address (`https://.../mcp`). If it requires a key, enter it under
**Headers**, one per line: `Authorization: Bearer ...`

To try one that needs no key, add `https://mcp.deepwiki.com/mcp`, which answers questions about public GitHub projects.

The address must use Streamable HTTP. Older addresses ending in `/sse` are not supported: ask the server's provider for
the newer address, which is often the same address ending in `/mcp`.

### By pasting JSON

Instructions written for Claude Desktop, Cursor or VS Code usually include a block of JSON. Open **Paste the JSON of a
server's instructions**, paste it, and press **Add these**. A server with a name already in use is replaced.

```json
{
  "mcpServers": {
    "memory": { "command": "npx", "args": ["-y", "@modelcontextprotocol/server-memory"] },
    "wiki": { "url": "https://mcp.deepwiki.com/mcp" }
  }
}
```

**Edit the list as JSON** shows the complete list in this format.

### Changing, disabling or removing a server

Each server in the list has a checkbox that disables it without removing it, as well as **Change** and **Remove**. The
wrench icon in the chat box also enables and disables servers. QuickAI's chat applies a change immediately; llama.cpp's
chat page applies it from the next Start.

## Servers to try

| What it gives the model | Command or address | Requirement | Tools |
| --- | --- | --- | --- |
| Read and change files in one folder | `npx -y @modelcontextprotocol/server-filesystem ~/Documents` | [Node.js](https://nodejs.org/) | 14 |
| Fetch a web page | `uvx mcp-server-fetch` | [uv](https://docs.astral.sh/uv/) | 1 |
| The time, in any time zone | `uvx mcp-server-time` | [uv](https://docs.astral.sh/uv/) | 2 |
| A notebook it can write to and read back | `npx -y @modelcontextprotocol/server-memory` | [Node.js](https://nodejs.org/) | 9 |
| Answers about public GitHub projects | `https://mcp.deepwiki.com/mcp` | nothing | 3 |

To find more, see the [list of servers](https://github.com/modelcontextprotocol/servers) and the
[MCP registry](https://registry.modelcontextprotocol.io/). Most servers are developed by third parties, so please read
[Stay safe](#stay-safe) first.

## Using tools in QuickAI's chat

**Checking which tools are enabled.** The corner of the wrench icon in the chat box shows how many servers are enabled,
in the warning color if one failed to start. Hover over it to see their names, or press it to open the list of your
servers, each with its number of tools, where you can enable or disable each one. The MCP servers page lists every tool
of a server under **What they offer**.

**Choosing when to be asked.** Each tool is set to **Ask** (QuickAI asks each time the model wants to use it),
**Always** (it runs without asking) or **Off** (it is not offered to the model). New tools are set to **Ask**.
Under **When the model uses a tool** on the same page, **Ask before a tool runs** can be turned off: every tool then runs
as soon as the model requests it, except those set to **Off**. Turn it off only for servers you trust, since their tools
then run without your review.

**Responding to the card.** When the model wants to use a tool, a card shows the tool and the input it would receive:

| Button | Effect |
| --- | --- |
| **Allow once** | Runs this one call. |
| **Always allow** | Runs it, and runs this tool without asking from then on (**What they offer** reverts this). |
| **Allow all tools** | Runs it, and every tool without asking from then on, except those set to **Off** (**Ask before a tool runs** reverts this). |
| **Deny** | Does not run it. The model is informed and continues without it. |

**Asking clearly.** The model decides whether to use a tool, so state what you want and give exact paths and
addresses. Asked for `shopping.txt` in a folder, Bonsai 2 4B passed the folder instead of the file; given the file's
full path, it succeeded.

**If responses are slow,** disable the tools you do not need: the description of every offered tool is sent with each
message. If a long chat runs out of context, choose **Long chats** in the panel on the right of the page.

**If the model does not use a tool,** ask it directly and check that the tool is enabled. If it still ignores tools,
or repeatedly calls them incorrectly, try another model: it must be one designed for tool use, and smaller models make
more mistakes. A model can request tools eight times in a row after a single message from you; the chat then stops and
waits for you. **Tool calls in a row**, under **When the model uses a tool**, raises this limit to 16, 32 or 64, or
removes it; without a limit, press **Stop** to end a chat in which the model keeps requesting tools.

**If the engine does not support tools,** the chat says so below the conversation and answers without them.

## Using tools in llama.cpp's chat page

With llama.cpp, **Open Chat UI** in the panel on the right (while the model is running) opens llama.cpp's own chat
page, which has the same servers. Press **+** beside its message box and choose **Tools**: **Server** lists the tools
of your servers, and the page asks **Allow once** or **Deny** for each call. Its own **MCP Servers** screen is a
separate list maintained by the page: servers added in QuickAI do not appear there, and you do not need to add them
again.

## Stay safe

- **A command server runs with your permissions.** It can do anything its program can do on your computer. Add only
  servers you trust. `npx -y` and `uvx` download the server from the internet and may pick up a newer version later.
- **Grant a server only what it needs.** Specify a single folder, not your entire home folder. The filesystem server
  can also write, edit and move files in that folder; set those tools to **Off** if you only want it to read.
- **Text returned by a tool can try to steer the model.** A web page could contain instructions aimed at the model,
  such as reading your files and sending them elsewhere. Review the input of a call before allowing it, and be
  cautious with **Always** when a web tool and a file tool are both enabled.
- **llama.cpp's confirmation belongs to the chat page; it is not access control.** While llama.cpp runs with servers,
  any program on this computer can run a tool directly through `http://127.0.0.1:<port>/tools` without being asked.
  QuickAI's server listens on this computer only, and it should stay that way: do not add `--host 0.0.0.0` under extra
  arguments while MCP servers are enabled. llama.cpp itself marks this feature as experimental and advises against
  exposing it to untrusted environments.
- **Some of what you send can leave your computer.** Your prompts normally remain on this computer, but whatever the
  model puts in a call is sent to the server. A server at an address receives it, along with your IP address, under its
  own terms, as does any service a command server contacts. With a model on another computer, a tool's output is sent
  there with the next message. See [PRIVACY.md](../PRIVACY.md).
- **Keys are stored on this computer.** They are kept in QuickAI's settings, which only your account can read, and are
  not included in configurations you share. Versions up to 0.2.1 left the settings file readable by other accounts on
  the computer: if you share a computer, update QuickAI before adding a key.

## Troubleshooting

Start with **What they offer** on the MCP servers page: a server that started shows **Ready** and its number of tools;
one that did not shows **Did not start** and the reason, in the server's own words where it provided any.

| What you see | What it means | What to do |
| --- | --- | --- |
| **Did not start:** `npx could not be started` | The program was not found. Your other servers still work. | Install it. If you started QuickAI from a desktop launcher, it may not see a program installed through your shell (nvm, fnm): use the full path, which `which npx` prints. |
| **Did not start:** `the server did not answer in 120 s` | The server is slow to start, or is waiting for something. | Run its command in a terminal to see its output. |
| **Ready · 0 tools** | The server provided no tools: a wrong address, a missing or incorrect key, or an older `/sse` address. | Check the address and the key. |
| A card that says **Failed** | The tool ran and reported a failure, or could not run. | The card states the reason. |
| No card when you ask | The model chose not to use a tool, or the server is disabled. | See *If the model does not use a tool* above. |

For llama.cpp's chat page, the log below the page (the log button in the toolbar) shows llama-server's own output: a
working server reports `MCP warmup: 'files' discovered 14 tools`, while `invalid argument: --mcp-servers-config` means
that the llama.cpp build is too old for MCP servers (choose a newer one under **Settings → Program**).

To query a server at an address yourself, run the following in a terminal, with its address at the end:

```bash
printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"check","version":"1"}}}' \
  | quickai mcp-bridge https://example.com/mcp
```

A reply containing `"result"` means the server answered; otherwise, the output explains why. For a key, prefix
`quickai` with `QUICKAI_MCP_HEADERS='{"Authorization":"Bearer ..."}'`. With an AppImage, use its path in place of
`quickai`.

## Limitations

- **ComfyUI.** It has no chat.
- **Connected applications.** They do not receive the tools automatically. With llama.cpp, an application can use them
  only by doing the work itself: `GET /tools` lists them, the application sends them with its own request as `tools`,
  and `POST /tools` with `{"tool": "files_read_text_file", "params": {...}}` runs one. The application is then
  responsible for asking you.
- **Resources and prompts.** QuickAI uses a server's tools, not the other capabilities MCP can offer.

---

The chat page's steps and the servers above were tested with QuickAI 0.2.1 (llama.cpp b11889) on Linux. QuickAI's own
chat with tools was tested with a stand-in engine and server on Linux. Windows and macOS have not been tested.

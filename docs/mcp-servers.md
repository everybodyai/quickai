# Give your model tools with MCP servers

On its own, a model can only write text. An **MCP server** gives it tools: read a file, fetch a web page, look
something up. (MCP is the Model Context Protocol, a common way of offering tools to a model.) A server is not a model:
you keep the model you use and add tools to it.

In QuickAI, MCP servers work:

- with the **llama.cpp** engine,
- on **this computer**,
- in **llama.cpp's chat page** in your browser. **The Chat tab inside QuickAI does not use them.**

## Try it: let a model read a file

This example starts its server with `npx`, so it needs [Node.js](https://nodejs.org/). The model in QuickAI's full
packages, Bonsai 2 4B, can do this.

1. Put a small text file in a folder, for example `notes.txt` in your Documents.
2. Open **Settings** (Ctrl+,) and choose **MCP servers**.
3. Fill in the form and press **Add**:
   - **Name:** `files`
   - **Command or address:** `npx -y @modelcontextprotocol/server-filesystem ~/Documents`
4. Close Settings and press the play button beside the model's name. If the model is already running, stop it and
   start it again: servers are read when it starts.
5. Press the **globe button** beside the model's name. The chat page opens in your browser.
6. Ask for something that needs the tool, with the file's full path:

   > Read the file /home/me/Documents/notes.txt and tell me what it says.

7. The page shows the tool the model chose (`files_read_text_file`) and what it wants to give it (**Input**), and asks
   **Allow use of files_read_text_file from Server Tools?** Check the **Input** and press **Allow once**.
8. The model gets the result and answers.

The first message takes the longest. `npx` may have to download the server, and the descriptions of this server's 14
tools add about 2,500 tokens. On an 8-core CPU that message took over a minute; the same request again took a few
seconds, because llama.cpp had already read that part.

## Add a server

Everything is under **Settings → MCP servers**. There are three ways, and a server's own instructions usually give you
what you need for one of them.

### By its command

Type the command as one line. Use quotes around a path with spaces:

```text
npx -y @modelcontextprotocol/server-filesystem "/home/me/My Notes"
```

If the server needs a key, put it under **Variables**, one per line: `API_KEY=...`

The command is not run in a shell, so `~` and `$HOME` reach the server as you typed them. The filesystem server
understands `~`. If another server does not, write the full path.

### By its address

Some servers run on the internet. Type the address (`https://.../mcp`). If it needs a key, put it under **Headers**,
one per line: `Authorization: Bearer ...`

To try one with no key, add `https://mcp.deepwiki.com/mcp`, which answers questions about public GitHub projects.

The address must be a Streamable HTTP one. An older address ending in `/sse` does not work: ask the server's maker for
the newer one, which is often the same address ending in `/mcp`.

### By pasting its JSON

Instructions written for Claude Desktop, Cursor or VS Code usually give a block of JSON. Open **Paste the JSON of a
server's instructions**, paste it, and press **Add these**. A server with a name you already use is replaced.

```json
{
  "mcpServers": {
    "memory": { "command": "npx", "args": ["-y", "@modelcontextprotocol/server-memory"] },
    "wiki": { "url": "https://mcp.deepwiki.com/mcp" }
  }
}
```

**Edit the list as JSON** shows the whole list in this form.

### Change, switch off or remove one

Each server in the list has a checkbox to switch it off without losing it, **Change** and **Remove**. Changes are used
from the next Start: stop the model and start it again.

## Servers to try

| What it gives the model | Command or address | You need | Tools |
| --- | --- | --- | --- |
| Read and change files in one folder | `npx -y @modelcontextprotocol/server-filesystem ~/Documents` | [Node.js](https://nodejs.org/) | 14 |
| Fetch a web page | `uvx mcp-server-fetch` | [uv](https://docs.astral.sh/uv/) | 1 |
| The time, in any time zone | `uvx mcp-server-time` | [uv](https://docs.astral.sh/uv/) | 2 |
| A notebook it can write to and read back | `npx -y @modelcontextprotocol/server-memory` | [Node.js](https://nodejs.org/) | 9 |
| Answers about public GitHub projects | `https://mcp.deepwiki.com/mcp` | nothing | 3 |

To find more, see the [list of servers](https://github.com/modelcontextprotocol/servers) and the
[MCP registry](https://registry.modelcontextprotocol.io/). Most are made by other people, so read
[Stay safe](#stay-safe) first.

## Use the tools in the chat page

**See which tools are on.** Press **+** beside the message box and choose **Tools**. **Server** is the tools of your
servers, with a count such as `14/14`. Open it to see each one, and use its checkbox to switch tools off. Their names
start with the server's name: `files_read_text_file`. **Browser** is two small tools of the page's own, not from your
servers.

**Ask plainly.** The model decides whether to use a tool, so say what you want and give exact paths and addresses. Asked
for `shopping.txt` in a folder, Bonsai 2 4B passed the folder instead of the file. Given the file's full path, it got it
right.

**Answer the question.** The page stops at each call until you answer, and the tool shows *Waiting for result...* until
then.

| Button | What it does |
| --- | --- |
| **Allow once** | Runs this one call. |
| The arrow beside it, then **Always allow** | Stops asking about this tool, or about all tools of your servers. |
| **Deny** | Does not run it. The model is told and goes on without it. |

**Ignore the page's own MCP Servers screen.** The icon in the page's left bar opens a screen that says *Add your first
MCP server*, even when your servers are working. That is a separate list kept by the page. The servers you added in
QuickAI do not appear there, and you do not need to add them again.

**If it is slow,** switch off the tools you do not need. The description of every tool that is on goes with each
message. If a long chat runs out of room, press **Change** beside the model in QuickAI for a larger context.

**If the model does not use a tool,** ask it directly and check that the tool is on. If it still ignores tools, or keeps
calling them wrongly, try another model: it has to be one made for tool use, and small models make more mistakes.

## Stay safe

- **A command server runs as you.** It can do whatever its program does on your computer. Add only servers you trust.
  `npx -y` and `uvx` download the server from the internet and can pick up a newer version later.
- **Give a server as little as it needs.** Name one folder, not your whole home folder. The filesystem server can also
  write, edit and move files in that folder; switch those tools off under **Tools** if you only want it to read.
- **Text from a tool can try to steer the model.** A web page could contain instructions aimed at the model, such as
  reading your files and sending them somewhere. Read the **Input** before you allow a call, and be careful with
  **Always allow** when a web tool and a file tool are both on.
- **The question is the chat page's, not a lock.** Any program on this computer can run a tool directly through
  `http://127.0.0.1:<port>/tools`, and it is not asked. QuickAI's server listens on this computer only, so keep it that
  way: do not add `--host 0.0.0.0` under extra arguments while MCP servers are on. llama.cpp itself marks this feature
  experimental and says not to expose it to untrusted environments.
- **Some of what you send can leave your computer.** Your prompts normally stay here, but what the model puts in a call
  goes to the server. A server at an address sees it, and your IP address, under its own terms. So does whatever service
  a command server contacts. See [PRIVACY.md](../PRIVACY.md).
- **Keys stay on this computer.** They are kept in QuickAI's settings and are never part of a configuration you share.

## If something does not work

Start with the **Log** tab under the model, and look for the lines that say `MCP`. A server that works says
`MCP warmup: 'files' discovered 14 tools`.

| What you see | What it means | What to do |
| --- | --- | --- |
| `MCP 'files': failed to spawn 'npx'` | The program was not found. Your other servers still work. | Install it. If you started QuickAI from a desktop launcher, it may not see a program set up by your shell (nvm, fnm): write the full path, which `which npx` prints. On Windows, try `cmd /c npx -y ...`. |
| `'wiki' discovered 0 tools` | The server gave no tools: a wrong address, no connection, a missing or wrong key, or an older `/sse` address. | Run the check below, which says why. |
| `MCP starting failed: ...` | llama-server could not read the list. | Correct it with **Edit the list as JSON**. |
| `invalid argument: --mcp-servers-config` | This llama.cpp is too old to know MCP servers. | Choose a newer build under **Settings → Program**. |
| No **Server** under **+ → Tools** | No server was loaded. | Stop the model and start it again, reload the page, and read the Log. |
| *Waiting for result...* on a tool | The page is waiting for you. | Press **Allow once** or **Deny** under it. |
| No **MCP servers** page in Settings | The engine in use is not llama.cpp. | Choose llama.cpp in the menu at the top right. |

To see why a server at an address gives no tools, run this in a terminal, with its address at the end:

```bash
printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"check","version":"1"}}}' \
  | quickai mcp-bridge https://example.com/mcp
```

A reply with `"result"` means it answered. Otherwise the output says why. For a key, put
`QUICKAI_MCP_HEADERS='{"Authorization":"Bearer ..."}'` before `quickai`. With an AppImage, use its path in place of
`quickai`.

## What it does not do

- **Other engines.** Ollama and ComfyUI have no MCP servers page.
- **Other computers.** The list stays on this computer and does not travel with a configuration, so a run on another
  computer does not have these servers.
- **QuickAI's Chat tab, or apps you connect.** They do not get the tools by themselves. An app can use them only if it
  does the work: `GET /tools` lists them, it sends them with its own request as `tools`, and
  `POST /tools` with `{"tool": "files_read_text_file", "params": {...}}` runs one. It is then responsible for asking you.

---

The steps and servers above were tried with QuickAI 0.2.1 (llama.cpp b11889) on Linux. Windows and macOS have not been
tried.

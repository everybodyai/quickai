# Give your model tools with MCP servers

On its own, a model can only write text. An **MCP server** gives it tools: read a file, fetch a web page, look
something up. (MCP is the Model Context Protocol, a common way of offering tools to a model.) A server is not a model:
you keep the model you use and add tools to it.

In QuickAI, MCP servers work:

- in **QuickAI's own chat**, with **llama.cpp** and **Ollama**, and with a model that runs on **another computer** you
  added. QuickAI runs the servers itself, on this computer, and asks you before a tool runs;
- in **llama.cpp's chat page** in your browser (**Open Chat UI**), which gets the same servers.

ComfyUI has no chat, so it does not use them. Apps you connect to the model do not get them either: they bring their
own tools.

## Try it: let a model read a file

This example starts its server with `npx`, so it needs [Node.js](https://nodejs.org/). The model has to be one made for
tool use; the one in QuickAI's full packages, Bonsai 2 4B, can do this.

1. Put a small text file in a folder, for example `notes.txt` in your Documents.
2. Press **MCP servers** in the sidebar.
3. Fill in the form and press **Add**:
   - **Name:** `files`
   - **Command or address:** `npx -y @modelcontextprotocol/server-filesystem ~/Documents`
4. Under **What they offer**, the server says **Starting…**, then **Ready · 14 tools**. The first time, `npx` downloads
   the server, which can take a minute.
5. Press the back arrow, then **Start**. The wrench in the chat's box, beside the clip, has a **1** in its corner: the
   model may use one server.
6. Ask for something that needs the tool, with the file's full path:

   > Read the file /home/me/Documents/notes.txt and tell me what it says.

7. A card shows the tool the model chose (**files · read_text_file**) and what it wants to give it. Check it and press
   **Allow once**.
8. The tool runs, the card says **Used** (**Show** opens what it was given and gave back), and the model answers with
   the file.

The first message takes the longest: the descriptions of this server's 14 tools go with it, about 2,500 tokens. On an
8-core CPU that took over a minute; the same request again took a few seconds, because the engine had already read
that part.

## Add a server

Everything is on the page of the **MCP servers**, in the sidebar. There are three ways, and a server's own instructions
usually give you what you need for one of them.

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

Each server in the list has a checkbox to switch it off without losing it, **Change** and **Remove**. The wrench in
the chat's box switches servers on and off too. QuickAI's chat uses a change at once; llama.cpp's chat page uses it from
the next Start.

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

## Use the tools in QuickAI's chat

**See which tools are on.** The wrench in the chat's box has how many servers are on in its corner, in the warning colour
when one did not start. Rest the pointer on it to see their names, or press it for the list of your servers, each with
how many tools it has, to switch each on or off. The page of the MCP servers lists every tool of a server under **What they offer**.

**Choose what is asked.** Each tool is **Ask** (QuickAI asks each time the model wants it), **Always** (it runs without
asking) or **Off** (the model is not offered it). A new tool is **Ask**.

**Answer the card.** When the model wants a tool, a card shows the tool and what it would be given:

| Button | What it does |
| --- | --- |
| **Allow once** | Runs this one call. |
| **Always allow** | Runs it, and this tool from now on, without asking (**What they offer** changes it back). |
| **Deny** | Does not run it. The model is told and goes on without it. |

**Ask plainly.** The model decides whether to use a tool, so say what you want and give exact paths and addresses.
Asked for `shopping.txt` in a folder, Bonsai 2 4B passed the folder instead of the file. Given the file's full path, it
got it right.

**If it is slow,** switch off the tools you do not need. The description of every tool that is offered goes with each
message. If a long chat runs out of room, choose **Long chats** in the panel at the right of the page.

**If the model does not use a tool,** ask it directly and check that the tool is on. If it still ignores tools, or keeps
calling them wrongly, try another model: it has to be one made for tool use, and small models make more mistakes. A
model can ask for tools eight times in a row after one message of yours; then the chat stops and waits for you.

**If the engine does not take tools,** the chat says so under the conversation and answers without them.

## Use the tools in llama.cpp's chat page

With llama.cpp, **Open Chat UI** in the panel at the right (while the model runs) opens llama.cpp's own chat page, which
has the same servers. Press **+** beside its message box and choose **Tools**: **Server** is the tools of your servers,
and the page asks **Allow once** or **Deny** at each call. Its own **MCP Servers** screen is a separate list that the
page keeps: the servers you added in QuickAI do not appear there, and you do not need to add them again.

## Stay safe

- **A command server runs as you.** It can do whatever its program does on your computer. Add only servers you trust.
  `npx -y` and `uvx` download the server from the internet and can pick up a newer version later.
- **Give a server as little as it needs.** Name one folder, not your whole home folder. The filesystem server can also
  write, edit and move files in that folder; set those tools to **Off** if you only want it to read.
- **Text from a tool can try to steer the model.** A web page could contain instructions aimed at the model, such as
  reading your files and sending them somewhere. Read what a call would be given before you allow it, and be careful
  with **Always** when a web tool and a file tool are both on.
- **llama.cpp's own question is the chat page's, not a lock.** While llama.cpp runs with servers, any program on this
  computer can run a tool directly through `http://127.0.0.1:<port>/tools`, and it is not asked. QuickAI's server
  listens on this computer only, so keep it that way: do not add `--host 0.0.0.0` under extra arguments while MCP
  servers are on. llama.cpp itself marks this feature experimental and says not to expose it to untrusted environments.
- **Some of what you send can leave your computer.** Your prompts normally stay here, but what the model puts in a call
  goes to the server. A server at an address sees it, and your IP address, under its own terms. So does whatever service
  a command server contacts. With a model on another computer, what a tool gives back is sent there with the next
  message. See [PRIVACY.md](../PRIVACY.md).
- **Keys stay on this computer.** They are kept in QuickAI's settings, which only your account can read, and are never
  part of a configuration you share. Versions up to 0.2.1 left the settings file readable by other accounts on the
  computer: if you share a computer, update QuickAI before adding a key.

## If something does not work

Start with **What they offer** on the page of the MCP servers: a server that started says **Ready** and how many tools
it has; one that did not says **Did not start** and why, in its own words when it gave some.

| What you see | What it means | What to do |
| --- | --- | --- |
| **Did not start:** `npx could not be started` | The program was not found. Your other servers still work. | Install it. If you started QuickAI from a desktop launcher, it may not see a program set up by your shell (nvm, fnm): write the full path, which `which npx` prints. |
| **Did not start:** `the server did not answer in 120 s` | The server is slow to come up, or waits for something. | Run its command in a terminal to see what it says. |
| **Ready · 0 tools** | The server gave no tools: a wrong address, a missing or wrong key, or an older `/sse` address. | Check the address and the key. |
| A card that says **Failed** | The tool ran and said it failed, or could not run. | The card says why. |
| No card when you ask | The model chose not to use a tool, or the server is off. | See *If the model does not use a tool* above. |

For llama.cpp's chat page, the log under the page (the log button of the bar) has llama-server's own lines: a server
that works says `MCP warmup: 'files' discovered 14 tools`; `invalid argument: --mcp-servers-config` means that build of
llama.cpp is too old for MCP servers (choose a newer one under **Settings → Program**).

To ask a server at an address yourself, run this in a terminal, with its address at the end:

```bash
printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"check","version":"1"}}}' \
  | quickai mcp-bridge https://example.com/mcp
```

A reply with `"result"` means it answered. Otherwise the output says why. For a key, put
`QUICKAI_MCP_HEADERS='{"Authorization":"Bearer ..."}'` before `quickai`. With an AppImage, use its path in place of
`quickai`.

## What it does not do

- **ComfyUI.** It has no chat.
- **Apps you connect.** They do not get the tools by themselves. With llama.cpp, an app can use them only if it does the
  work: `GET /tools` lists them, it sends them with its own request as `tools`, and `POST /tools` with
  `{"tool": "files_read_text_file", "params": {...}}` runs one. It is then responsible for asking you.
- **Resources and prompts.** QuickAI uses a server's tools, not the other things MCP can offer.

---

The chat page's steps and the servers above were tried with QuickAI 0.2.1 (llama.cpp b11889) on Linux. QuickAI's own
chat with tools was tried with a stand-in engine and server on Linux. Windows and macOS have not been tried.

# Under the hood

## What's running her

[Synaps](https://github.com/HaseebKhalid1507/SynapsCLI): an open-source agent runtime, written in Rust.
One background daemon owns every session. Capri is a client of that daemon with a session of her own,
named `capri`. That's why she picks the conversation back up after a restart, and why other things can
join it.

She's the body. Synaps is the brain. You already have both.

## Three things to try

**1. Open her thread in a terminal.**

```sh
synaps --attach capri
```

The same conversation, in the Synaps TUI, with her tool calls and their output in full. Type in either
window. Watch her while you do.

**2. Make her come and get you.**

```sh
sleep 240; synaps send --session capri "tea's ready"
```

Four minutes later she comes over and tells you. Anything that can run a command can do this: a build,
a backup, a cron job. Each message is one turn of her model. [WATCH.md](WATCH.md) is the same trick,
packaged.

**3. Start a session that's yours.**

```sh
synaps
```

A fresh session, in the terminal. Hers is listed when you open it. `/sessions` switches between them.

## Her brain

She uses whichever model your Synaps is logged in to. Until it has one her tag reads NO BRAIN YET,
and her answer to your first message ends on the command. Log in, or change it:

```sh
synaps login --provider claude     # a Claude subscription: it opens your browser
synaps login                       # or pick a provider: a subscription, an API key
```

To give her a model of her own, set it in `~/.config/synaps-avatar/settings.toml`:

```toml
[session]
model = "anthropic/claude-opus-5"   # provider/model, as Synaps names them
```

No account? A model on your own machine works, through [Ollama](https://ollama.com/download):
Synaps finds it at `localhost:11434` by itself. She needs a model with tool calling, and room for her
prompt (~7k tokens: her tools and her persona). Ollama gives a model a 4k context unless your GPU
has 24 GB or more, and cuts a longer prompt without a word. So, once Ollama runs:

```sh
ollama pull qwen3:8b                                           # ~5 GB
printf 'FROM qwen3:8b\nPARAMETER num_ctx 16384\n' > /tmp/Modelfile
ollama create capri-qwen3 -f /tmp/Modelfile                    # the same model, a 16k context
```

then in her settings:

```toml
[session]
model = "local/capri-qwen3"
```

and `avatarctl settings reload` (on Windows: `%APPDATA%\synaps-avatar\settings.toml`, then restart
her). A local model makes a slower, plainer Capri, and without a GPU a slow one. When it fails she
says why: Ollama off, a model it hasn't pulled, or one that can't take her request.

## What else Synaps does

- **Sessions outlive the window.** The daemon owns them. Close every terminal and they keep going.
- **Several windows, one session.** Any attached client can type, steer or take over.
- **A crew.** Named subagents working in parallel. Open one and steer it mid-task.
- **Any model.** Claude, Codex, Grok, Copilot, Gemini, Kimi, any OpenAI-compatible endpoint, or Ollama.
- **Your keys stay put.** Credentials live in a broker. Agents get short-lived tokens, never the keys.
- **Extensions in any language.** A folder in `~/.synaps-cli/plugins/`, its own process, its own permissions.
- **Small.** One binary, around 32 MB.
- **Yours.** Apache 2.0. The system prompt is a file on disk. Nothing phones home.
- **21 themes.** One of them is `night-city`.

All of it, with the numbers: [SynapsCLI's README](https://github.com/HaseebKhalid1507/SynapsCLI#what-it-does).

## What she can touch

Capri is a real agent, not a chat window. Her session has Synaps's tools: it can read and write files and
run commands, as you.

- **Synaps is not a sandbox.** Tools run with your permissions.
- **Prompts** (the ALLOW / DENY box, hold <kbd>A</kbd>) appear only when something asks for one: an
  extension that guards tool calls, or one that needs a secret. Whether anything asks before a command
  depends on what your Synaps has installed. Don't count on a prompt you haven't seen.
- **She never approves a prompt for you**, and events from `capri-watch` never make her act on her own:
  anything risky waits for your yes.
- **Want her to only talk?** Take the tools away in `~/.synaps-cli/config`:
  `disabled_tools = bash, shell_start, write, edit, multi_edit` (the built-ins that run commands or
  change files). That applies to every session on that daemon.
- **Doing something you don't trust?** Use a container or a VM. Synaps says the same.

## Leaving

`install.sh --uninstall` removes her. Synaps stays, because other clients of its daemon may use it: the
binary is `~/.cargo/bin/synaps` and its data is in `~/.synaps-cli`. The uninstaller's last lines say
where Synaps is and what to delete if you don't want it either.

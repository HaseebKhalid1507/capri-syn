# Installing her

One script per OS, the same seven steps and the same flags: `install.sh` (Linux) and `install.ps1`
(Windows). Each step checks before it acts, so a rerun is an upgrade, and `--dry-run` prints every
action without doing any of them. Their pins and URLs (Synaps' minimum version and hashes, the
release asset names, CapriCore's commit, the package lists) live in one file both read,
[`installer/manifest`](../installer/manifest).

## Linux

```sh
curl -fsSL https://raw.githubusercontent.com/HaseebKhalid1507/capri-syn/main/install.sh | bash
# with flags:
curl -fsSL https://raw.githubusercontent.com/HaseebKhalid1507/capri-syn/main/install.sh | bash -s -- --voice --yes
# from a checkout (builds this tree):
./install.sh
```

Run it as yourself, not with sudo: it refuses root unless `--allow-root` (and always when `HOME` is
someone else's, as `sudo` leaves it). It never runs `sudo` on its own. It prints the exact package
command, then asks; `--yes` answers for it. The prebuilt binary is x86_64 with glibc 2.35 or newer;
on anything else (an older glibc, another architecture) it builds her from source by itself. Tested on
Debian 12, Ubuntu 24.04, Arch and Fedora (see below). openSUSE (zypper) is wired, not tested.

## Windows

```powershell
irm https://raw.githubusercontent.com/HaseebKhalid1507/capri-syn/main/install.ps1 | iex
# with flags (iex can't pass them):
& ([scriptblock]::Create((irm https://raw.githubusercontent.com/HaseebKhalid1507/capri-syn/main/install.ps1))) --voice --yes
# or from the environment (read when given no flags, said in the banner, then cleared):
$env:CAPRI_INSTALL_FLAGS = '--voice --yes'; irm https://raw.githubusercontent.com/HaseebKhalid1507/capri-syn/main/install.ps1 | iex
```

Windows 11, x64. Windows 10 (1809 or newer) and Windows 11 on Arm (x64 emulation) get through both
installers, but she has never been run on either: untested, and not supported yet
([WINDOWS.md](WINDOWS.md)).

`CAPRI_INSTALL_FLAGS` applies to one run: install.ps1 removes it from your session once read, so a
later bare `irm | iex` can't pick up an old `--uninstall`. (On Linux use `bash -s -- FLAGS`; a
script can't clear a variable you exported in your shell.)

`irm | iex` runs a script in your session's scope. This one runs as a module of its own: it leaves
no variables, functions or preference changes behind (CI checks that).

Per user, no admin (except the C++ build tools a source build needs, behind a UAC prompt). Every
flag also has PowerShell's spelling: `-Yes`, `-DryRun`, `-NoAutostart`, and so on.

Or her installer, no terminal: `capri-syn-setup-x86_64.exe` from the latest release. Double-clicked,
it does what this script does (Synaps when it's missing, her art downloaded and built, the daemon,
autostart, her voice if you tick it) through the same steps (`packaging/windows/steps.ps1`), and
its "Her model" page asks which brain, as this script does:
[docs/WINDOWS.md](WINDOWS.md#the-installer).

## The steps

| # | step | Linux | Windows |
|---|---|---|---|
| 1 | **Synaps** | When `synaps` is missing or older than `synaps_min` (0.11.0: her client speaks protocol 3), it says what Synaps is (her brain: an open-source agent runtime, yours to keep or remove) and which of your shell files its installer will add a PATH line to (`. "$HOME/.cargo/env"` in `~/.profile`, the bash rc files you have, a zsh one, fish's `conf.d`; none when `~/.cargo/bin` is on your PATH already), then **asks** (`--yes` is the consent; declined, it stops and prints how to install it yourself with `SYNAPS_NO_MODIFY_PATH=1`). Then it runs SynapsCLI's own `synaps-installer.sh` from the release the manifest pins (`synaps_pin`), checked against the manifest's sha256 of it. Never the latest: her client speaks one protocol exactly. | `synaps-installer.ps1` of the release the manifest pins (`synaps_pin`, one she was run against), checked against its pinned SHA-256; never the latest (a daemon on another protocol fails the daemon step). The edit it names is `~\.cargo\bin` on your user `Path`. |
| 2 | **Her binary** | The release tarball (`synaps-avatar-x86_64-unknown-linux-gnu.tar.gz`), checked against its `.sha256`. With no release binary for this machine (none published yet, not x86_64, or a glibc older than 2.35), or with `--from-source`: rustup if missing (`rustup-init` checked against its sha256, `--no-modify-path`), the toolchain `rust-toolchain.toml` pins, and `cargo build --release --locked`, in `~/.cache/synaps-avatar/installer`. Before that it prints the distro's packages (apt, pacman, dnf or zypper), the `-dev` libraries for a source build and the runtime ones for a binary, then asks. It checks for a Vulkan driver and only says which one to install. | Her installer (`capri-syn-setup-x86_64.exe`), checked against its `.sha256`, run unattended (`/SYNAPS=skip /PACK=skip`: those are this script's steps). Without one: rustup, Visual Studio's C++ build tools (winget, asks), and a windowed build. |
| 3 | **Her art pack** | `tools/make-pack`: CapriCore v1.0.4 plus lachy's desktop art at their pinned commits, every file checked against `tools/*.sha256`, built with Python and fonttools in a venv of its own (never system-wide). It prints lachy's licence and Required Notice lines. A pack built from the same inputs is left alone. Mochi's art is **not** fetched: `--with-local-art` adds it, for local personal use only. | The same make-pack and pins, run by `packaging/windows/steps.ps1` (her installer's "Download her art" runs the same script) in CPython's embeddable package and pip, pinned and SHA-256 checked, in `%LOCALAPPDATA%\synaps-avatar\installer`: no Python of yours needed or touched. |
| 4 | **Daemon and her session** | When systemd's user manager runs, it writes and enables a `synaps-daemon.service` user unit; a unit of your own is kept, and enabled only if you agree. With no user manager, it starts the daemon now, and her autostart line starts it at login. It checks the daemon speaks protocol 3; a daemon that speaks another, or doesn't say, stops the install (exit 1, with how to fix it: an older daemon still running after this run installed or upgraded Synaps is stopped, `synaps daemon stop`; an upgrade says so in step 1 too). Her session `capri` is her own, as before (attached if live, else resumed, else made). | It starts `synaps daemon --detach` now, plus a Run value (`synaps-daemon`) for sign-in. A `synaps-daemon` value of your own is left as it is and never recorded. It checks the daemon speaks protocol 3; one that speaks another, or doesn't say, stops the install (exit 1, with how to fix it: after this run installed or upgraded Synaps, it's an older daemon still running, `synaps daemon stop`; an upgrade says so in step 1 too). Her installer finishes anyway and says it on its finish page. |
| 5 | **Summon key** | i3, sway or Hyprland: shows the binding (`$mod+c`: press and release, so a hold talks). With your consent (`--yes`) it appends it between marker comments, after a timestamped backup. The key is named as your config sets `$mod` (Super for Mod4, Alt for Mod1). One that is already taken is reported, not overridden; on Hyprland (whose stock config closes a window on Super+C) it offers Super+Alt+C instead, Windows' key, when that one's free. GNOME: it offers a custom keyboard shortcut, Super+Alt+C running `avatarctl talk tap`, only when you say yes on the terminal ([below](#gnome-kde-and-other-wayland-desktops)). On any other desktop it tells you what to bind (`avatarctl talk tap`). | Win+Alt+C: she registers it herself. |
| 6 | **Autostart** (default on) | i3 and sway get an `exec` line, Hyprland an `exec-once`, others an XDG autostart `.desktop`. Each one starts the daemon first if needed. Whatever `--no-autostart` says, she also goes in your app menu as Capri (`~/.local/share/applications/synaps-avatar.desktop`, with her icon), so once she's quit she starts from there. | A Run value (`synaps-avatar`). |
| 7 | **`--voice`, `--watch`** | `--voice` runs `tools/capri-voice-setup` (its own venv and ~375 MB of models, and ~105 MB for Python 3.13 if your system has none of 3.10 to 3.13), installs `capri-voice` (run by hand it uses that venv's Python) and `capri-voice.service`. `--watch` installs `capri-watch.service`, enabled only with `--yes`. | `--voice` uses her voice installer (or builds the same bundle without a release) and fetches the models. `--watch` is Linux-only. |

At the end comes an `installed:` summary: what is where, how the daemon starts, how to summon her,
how to start her, and how to remove her (as you ran it: the checkout's script, or the piped form).

**`--voice` on Linux needs** PipeWire's `pw-record` and `pw-play`, and a Python 3.10 to 3.13 for its
venv (kokoro-onnx doesn't take 3.14, which is Arch's `python3` now). The first is checked before
anything is installed, and never installed for you; the package is `pipewire-bin` on Debian and Ubuntu,
`pipewire-utils` on Fedora, `pipewire-audio` on Arch (not `pipewire`: it has neither tool) and
`pipewire-tools` on openSUSE. For the second, `tools/capri-voice-setup` uses your `python3` when it is
a CPython 3.10 to 3.13 (the distro's own: its `pywhispercpp` goes with it), else a `python3.13` to
`python3.10` on your PATH; with none, uv fetches Python 3.13 (a 34 MB download, 105 MB on disk) into
`~/.local/share/capri-voice/python`, inside the voice's own folder (whatever `python-preference` uv has
for you). install.sh puts uv in its own venv when you have none. Run the setup by hand with no uv and
no such Python, and it stops with one line: install `uv` (Arch: `pacman -S uv`) or `python3.13` (Arch:
`python313` from the AUR). Started by hand (no systemd), `capri-voice` runs on the voice's venv: its
first line is that venv's `python`.

When Synaps has no model yet (no login in `~/.synaps-cli/auth.json`, no `local/` model in its config
or her `[session]`), it asks which brain, in your terminal. None is picked for you; Enter is later.

1. **Sign in to an account**: ChatGPT (a free account works, as the list says: her model is then
   its default, GPT-5.6 Terra, which a free account runs), Claude, Copilot, Gemini (when Synaps has
   its Google client), Grok, Kimi or Muse: her own table's (`synaps-avatar --providers`,
   `src/provider.rs`' `ALL` in its order, a hint beside a name its last field), the same as her
   Settings ▸ Brain; the installers keep no copy. It runs
   `synaps login --provider …` here, and your browser opens on the sign-in page by itself; Copilot's
   page asks for the code Synaps prints. Nothing to copy. Once `auth.json` is written, its stdin (a
   pipe held open: Synaps 0.12 gives up on a closed one, and asks about its app on a terminal) is
   closed and it ends: no wait. Ctrl-C stops it too.
2. **A model on this machine**: the models Ollama (`:11434`) and LM Studio (`:1234`) serve now, if
   either runs (asked directly, never through a proxy; embedding models left out, and any id with
   more than letters, digits and `._:/@+-`). With none, it says so (Ollama: ollama.com) and picks
   later. It downloads nothing.
3. **An API key**: Groq, OpenRouter and the other key services. The key is read without echo and
   goes to `synaps login` on its stdin: never in an argument, a log or on screen.
4. **Later**: her tag reads NO BRAIN YET, her first words end on a red `SET ME UP ▸`, and while she
   has none a click on her or her summon key opens the Line on **Settings ▸ Brain**: three ways in,
   none first. SIGN IN lists the same accounts; a click runs Synaps' sign-in for it with no window and
   your browser opens on the provider's page (Copilot's code is shown large and copied, to type there;
   on Windows Grok needs a newer Synaps). API KEY takes a key in a hidden field and hands it to Synaps
   on its stdin. RUN LOCAL lists your local server's models when it's up with some, else opens the
   local-model steps ([UNDER-THE-HOOD.md](UNDER-THE-HOOD.md#her-brain)). Signed in, she switches to
   that account's model. With no model picked anywhere and no login, her failed first message says to
   pick a brain (`SET ME UP ▸`), not to sign in to Claude.

Then her model is that one: an account's or a key service's default (her own table's,
`synaps-avatar --providers`; an account's once you're signed in, never before), or `local/<model>`,
through `synaps-avatar --set session.model=…` (a running her reads her settings again). A
model on LM Studio also sets Synaps' `provider.local.url` (Ollama's is its default) and reloads the
daemon. `--brain` picks without asking (unattended: `--brain=later`, `--brain=claude`,
`--brain=local:qwen3:8b`), and with `--yes`, `--dry-run` or no terminal it's later unless `--brain`
says otherwise. A key needs your terminal. The daemon picks a login up on her next turn, with no
restart: Synaps reads `auth.json` per request. Her installer on Windows asks the same on its
"Her model" page ([WINDOWS.md](WINDOWS.md#the-installer)). When Synaps was already there with a
model, one line says her session is `capri`.

Then it starts her, `--no-autostart` or not (that flag is about login), the way autostart will
(Linux: `env SA_BACKEND=x11` off Hyprland and i3, so GNOME and KDE on Wayland run her through
XWayland), from your home folder, in a session of her own so closing the terminal leaves her be, and
waits up to 20 s for her first frame (`~/.local/state/synaps-avatar/launch.log`, the run before's
kept as `launch.old.log`; Windows: her `synaps-avatar.log`). Either one starts over past 4 MB while
she runs, the earlier part kept as `.old.log`.

- **One already running as autostart starts her** (no arguments: the old binary) is quit and started
  again.
- **One you started with your own arguments** (`--socket`, `--profile`, `--session`...) is left
  running, and it tells you to quit her, then start her. On Windows her binary can't be replaced
  while she runs, so that one stops anyway; it isn't replaced by a bare one, and you get its own
  command line back to start it again.
- **On Windows, every run of yours stops for a new binary**, in any sign-in session (one on your
  desktop, seen from an SSH install, too), each named as it stops (its pid, its session, its command
  line when it's your own run). One stopped in another session is said again at the end: start her
  there again from the Start menu.
- **When her binary is replaced while she runs**, the step says now what the end will do: restart
  her, leave your own run be, or not restart her from here (SSH, no desktop).
- **No desktop here to show on**, so it doesn't start her and says so: an SSH session
  (`SSH_CONNECTION` or `SSH_TTY`: with `ssh -X` she'd open on the far screen and die with the
  tunnel), no `DISPLAY` or `WAYLAND_DISPLAY` (CI), or no interactive session on Windows.

The `start her:` line is the same command, to start her again later.

A pack at her pack's place that this script didn't make (your own build) is moved aside to
`capri.pre-install-<time>`, said so, and never removed by `--uninstall`.

## GNOME, KDE and other Wayland desktops

Hyprland is the one Wayland desktop she runs on natively. On any other (GNOME, KDE Plasma, sway...)
she runs through XWayland, the X server those desktops keep for X apps.

- **She picks it herself.** A bare `synaps-avatar` with `WAYLAND_DISPLAY` set, no Hyprland and
  XWayland running (`DISPLAY` set) runs as `SA_BACKEND=x11` does, and says so in one line of her
  log, after its header (when she starts: not for `--check` or `--set`). Without XWayland (`DISPLAY`
  unset) she stops with an error that says so: turn XWayland on, or log in to an X11 session.
  `SA_BACKEND` always wins, in any case: `x11` is XWayland (on Hyprland too), `wayland` is Hyprland
  or nothing (with no Wayland session at all, an error); any other value is ignored, with a line in
  her log. The installer's start line, autostart entry and app-menu entry still say
  `env SA_BACKEND=x11` off Hyprland and i3.
- **Hyprland is** a `HYPRLAND_INSTANCE_SIGNATURE` naming one that runs on this display, or
  `XDG_CURRENT_DESKTOP` or `XDG_SESSION_DESKTOP` saying Hyprland while one does; the installer reads
  it the same way. One runs while the PID in its lock
  (`$XDG_RUNTIME_DIR/hypr/<signature>/hyprland.lock`) is a Hyprland that runs (its process name says
  Hyprland, as Nix's `.Hyprland-wrapped` does too): a crash leaves the lock and its sockets behind,
  and its PID may be another process's by now. It serves the display its lock names, which has to be
  `WAYLAND_DISPLAY`. No lock: Hyprland's first moments (its request socket comes up before the
  lock), or a shutdown that crashed after removing it. Her binary counts it while the socket takes a
  connection, as it does a Hyprland whose PID is in another PID namespace (tools/wlrig); the
  installer can't connect to a socket, and doesn't. Hyprland before 0.40 lived in `/tmp/hypr`,
  which she doesn't read: unsupported (she takes XWayland, and her log calls its signature left
  over).
  - Left over from an earlier Hyprland login (in GNOME's environment, or a tmux server's, which
    keeps the ones it started with), a signature or desktop variable names no Hyprland that runs
    here; a compositor nested in Hyprland inherits them, and serves a display of its own. Either way
    she takes XWayland, and her log says what she ignored.
  - Hyprland started where its signature got lost on the way to her (systemd or D-Bus activation)
    still reads as Hyprland, and her error says it's missing rather than her taking XWayland
    unasked.
- **What she can't do there:**
  - she perches only on X apps: a native Wayland window doesn't exist for her, so she can't sit on
    it or follow it;
  - she sees the pointer only while it's over an X window, so cursor play stops over native ones;
  - presence (how long since you last typed or moved the mouse: XScreenSaver's idle time) counts
    only input that X apps get. While you type in a native Wayland app she may decide you're away
    and nap.
- **Her summon key on GNOME** is a custom keyboard shortcut. The installer checks that Super+Alt+C
  is free (GNOME's window, Shell and media-key shortcuts, every custom one, and the Shell
  extensions': the system's schemas and those of the ones you installed yourself, each folder's own,
  in `~/.local/share/gnome-shell/extensions`; Mod4 is Super and Mod1 Alt), then asks on the terminal
  whether to add it, running `avatarctl talk tap`. The default is no; `--yes` doesn't answer it, and
  with no terminal it isn't asked. Said no, or the key is taken, it tells you where to add one
  yourself: Settings > Keyboard > Keyboard Shortcuts > Custom Shortcuts. It's added to the end of
  your list (yours stay), and `--uninstall` takes it out again (unless you changed it to run
  something else; where gsettings doesn't run, it stays in the receipt for an `--uninstall` where it
  does). Both read GNOME's settings back after writing (`gsettings set` can succeed while dconf
  keeps nothing): when they don't hold it, you get a warning and the exact `gsettings` commands to
  run yourself, and `--uninstall` keeps that line in its receipt so the next one tries again. On
  KDE, bind `avatarctl talk tap` in its Shortcuts settings yourself.

## Known issues

- **A dark slab on X11 without a compositor**
  ([#72](https://github.com/HaseebKhalid1507/capri-syn/issues/72)): now and then her whole window's
  rectangle paints solid black behind her and her bubble, covering what's under it, more often while
  a prompt hangs below her. So far it has only been seen with no compositor running, where she cuts
  her window to her outline with the X shape. Workaround (likely; not yet confirmed): run a
  compositor (picom: `picom -b`).
  With `[display] composited = "auto"`, the default, she switches to alpha when it starts, no
  restart.
- **GNOME, KDE and sway** run her through XWayland, with its limits ([above](#gnome-kde-and-other-wayland-desktops)).
  None of them has been tried on a real session yet: CI and the test containers stand in for them.
- **Hyprland** has no idle time for her yet: your presence is the pointer moving, so typing alone
  for 5 min reads as away. Hyprland before 0.40 isn't supported: she takes XWayland there.
- **On Windows, Synaps 0.12's own `synaps login`** opens a sign-in tab cut at the URL's first `&`:
  copy the whole URL it prints into your browser, or sign in through her installers' step, which
  opens it whole (install.ps1 puts it there too; the default folder shown; `-Provider` for another
  account than Claude):
  `powershell -NoProfile -ExecutionPolicy Bypass -File "$env:LOCALAPPDATA\Programs\synaps-avatar\steps\steps.ps1" login`
  Grok's sign-in never prints its URL, so the Windows installers don't offer Grok yet
  ([WINDOWS.md](WINDOWS.md#the-installer)).
- **Windows:** the installers are unsigned, so a browser download meets SmartScreen ("More info",
  then "Run anyway"); Windows 10 and Windows 11 on Arm are untested; a click on one of her notifications does nothing (as
  on Linux); her voice brings its own VC++ runtime but hasn't yet run on a PC without the Visual C++
  Redistributable; live audio is still to check on real hardware: her desktop checks ran in a
  Windows 11 VM, with no real audio ([WINDOWS.md](WINDOWS.md)). Linux has no tray icon yet: HIDE's
  chip is the way back there.
- **Windows: her context doesn't roll over yet.** Synaps 0.12 keeps its context archive on Linux and
  macOS only, so `[session] rollover` ([REFERENCE.md](REFERENCE.md#the-session-rules)) is skipped there
  (her log says so). A long chat fills her model's context until a turn is refused as too long; then
  compact or clear it from Synaps (`/compact` or `/clear` in `synaps --attach capri`).
- **`XDG_RUNTIME_DIR` unset:** she and her voice sidecar look for the Linux socket in different
  places: she in the temp dir (`$TMPDIR`, else `/tmp`), the sidecar in `/run/user/<uid>`. Set it, or
  set `CAPRI_VOICE_SOCK` for both. It isn't aligned yet because that would change how she behaves.

## Flags

| flag | |
|---|---|
| `--yes` | don't ask: Synaps' installer and its PATH edits, sudo for packages, edits to the window manager's config, enabling capri-watch (Windows: winget). Her brain is later (unless `--brain`): it never opens `synaps login`, and never adds a GNOME shortcut (that one takes your own yes) |
| `--brain=WHICH` | her brain, unasked, when Synaps has none: an account (`claude`, `openai-codex`, `github-copilot`… its provider id or login), `local` (the first model running here) or `local:MODEL`, a key service (`groq`…: asks for the key, so it needs your terminal), or `later` |
| `--dry-run` | print every action, touch nothing |
| `--uninstall` | remove what it installed; settings, state and her log stay (on Windows her installer's own uninstaller takes her state and log) |
| `--purge` | with `--uninstall`: remove those too: her own files only (`settings.toml`, `state.json`, `sessions.json`, her log, the installer's cache), never the folders' other contents. A pack it didn't make (yours at `packs/capri`, or one moved aside) is never removed |
| `--voice` | her voice too |
| `--watch` | capri-watch too (Linux) |
| `--no-autostart` | don't start her at login (she still starts at the end of the install) |
| `--from-source` | build even when a release has a binary |
| `--prefix DIR` | where `bin/` and `lib/synaps-avatar/` go (Linux, default `~/.local`); her folder (Windows, default `%LOCALAPPDATA%\Programs\synaps-avatar`) |
| `--with-local-art` | mochi's art too: **local personal use only**, never publish or share it |
| `--artifact FILE` | install this release asset instead of downloading one (tests, offline) |
| `--allow-root` | install her for root (Linux; as root, with root's own `HOME`). Without it, root is refused |

With no flags, both read `CAPRI_INSTALL_FLAGS` (space-separated), for `irm | iex`, and say so in
the banner. install.ps1 then clears it from your session.

## What it writes, and uninstalling

Every file, folder, unit, Run value and config block it adds is listed in a receipt
(`~/.local/share/synaps-avatar/install.receipt`; Windows: `%LOCALAPPDATA%\synaps-avatar\install.receipt`),
one entry per line, tab-separated, the path last (`file PATH`, `block TAG FILE`, `unit NAME FILE`...),
so a path with spaces stays one path. `--uninstall` walks it backwards:
- it stops her;
- it disables the units it enabled;
- it removes the marker blocks, keeps your config's backup, and leaves a symlinked config a symlink
  (a config that would be left empty is left as it is);
- it takes her GNOME shortcut out of your custom shortcuts and resets it (`gkey PATH`; one you made
  run something else stays);
- it removes the folders it made (when empty), the prefix's included.

What didn't come out (her GNOME shortcut, when gsettings doesn't take it out or isn't there) stays
in the receipt, with the folders it made that are still there and the prefixes she went to, and the
next `--uninstall` tries again.

It removes only what is inside her folders (each `--prefix` she went to, and her data, config and
cache folders): never `/` or your home, nothing with a newline or `..` in it. A folder removed
whole must be one it makes (her pack, `lib/synaps-avatar`, the installer's cache, the voice
folders). A unit, her app-menu entry and her autostart entry are removed only while they still
carry the header it wrote. An entry outside those is reported and left alone. (KDE's menu editor
keeps no comments, so an entry you edited there is yours now: it stays, and her autostart entry's
`TryExec` keeps one left behind from starting anything once she's gone. An install that finds an
entry there without the header backs it up first, as it does your configs.)

Synaps stays installed, since her daemon may have other clients, and so does rustup if it installed
it. `--uninstall` says where (`~/.cargo/bin/synaps`) and that your Synaps login and sessions live in
`~/.synaps-cli` (Windows: `%USERPROFILE%\.synaps-cli`). When this script installed Synaps (there was
none before), it also says how to remove it: `synaps daemon stop`, then delete the binary and Synaps'
install receipt (`~/.config/synaps`; Windows `%LOCALAPPDATA%\synaps`), `~/.synaps-cli` only if you
want your login and sessions gone, and which shell files got its PATH line (Windows: `~\.cargo\bin`
on your user `Path`); keep that if you use Rust. A Synaps you had, or one it upgraded, is yours:
it only says to uninstall it the way you installed it.

Linux:
- `~/.local/bin/{synaps-avatar,avatarctl}` and `~/.local/lib/synaps-avatar/`: her binary and tools;
- `~/.local/share/synaps-avatar/packs/capri`: her pack;
- `--voice`: `~/.local/bin/capri-voice` and `~/.local/share/capri-voice/` (its venv, ~375 MB of models,
  and the Python 3.13 uv fetched, if it did);
- `~/.cache/synaps-avatar/`: the source, the build and the make-pack venv (reused by reruns);
- `~/.config/systemd/user/`: the units;
- `~/.local/share/applications/synaps-avatar.desktop` (her app-menu entry) and
  `~/.config/autostart/synaps-avatar.desktop` (off i3, sway and Hyprland).

Windows: her installer's folder, her pack under `%LOCALAPPDATA%\synaps-avatar`, and the Run values.

## Releases

`.github/workflows/release.yml` runs on a `v*` tag. The tag must match `Cargo.toml`'s version, and
`CHANGELOG.md` must have a `## <version>` section: it becomes the release's notes
(`installer/release-notes.sh`), so a tag without one fails before anything is built. It builds:
- the Linux tarball, on ubuntu-22.04 for glibc 2.35: her binary, avatarctl, the tools and units,
  and `install.sh`;
- her Windows installer and her voice installer.

Each asset gets a `.sha256`, is smoke-tested through the installers above, and is attached to that
tag's release with `install.sh`, `install.ps1` (that commit's), espeak-ng's source (below),
`SHA256SUMS` and a build-provenance attestation (`gh attestation verify FILE --repo
HaseebKhalid1507/capri-syn`). From v1.0.0 every asset name is frozen: installers, docs and the voice
installer's `SOURCE.txt` point at them. If there is no
release yet, it creates a **draft**: publishing is a person's call, and `releases/latest` (what the
installers read) skips drafts. Until a release exists, both installers build from source.

**Licences inside the artifacts.** Each one carries capri-syn's `LICENSE` and the notices that go
with what's in it:
- the tarball (under `lib/synaps-avatar/`, installed with her) and her Windows installer (under her
  folder): `licenses/` holds GPUI's Apache-2.0 and its `PATCHES.md`, synaps-client's `NOTICE` and
  Apache-2.0, the icon's `NOTICE`, Lucide's ISC licence (its icons are embedded by gpui-kit-assets,
  which declares only Apache-2.0), and `THIRD-PARTY-LICENSES.txt`, every crate compiled into that
  build with its licence text (cargo-about, pinned: `installer/licenses/`). The list is
  `installer/licenses/list`. The generation stops on a licence not accepted in `about.toml` (MPL-2.0
  crate by crate), on a crate with no copyright line that isn't known, on a gpui-kit-assets that
  isn't the one Lucide's licence was copied for, and if syntect, two-face or tree-sitter come back (she
  highlights no code, and their syntax definitions and queries carry licences of their own);
- the wake-word model's `NOTICE` beside it (`tools/wake/`, in the tarball and the voice installer);
- her voice installer: `licenses/THIRD-PARTY-LICENSES.txt` (every Python package in it with the
  licence texts its wheel carries, and Microsoft's terms for the Visual C++ runtime), espeak-ng's
  licence files (with sonic's, compiled into its DLL) and `SOURCE.txt` (where the GPL parts' source is: espeak-ng 1.52.0, phonemizer),
  whisper.cpp's licence, PortAudio's, and CPython's own (`python/LICENSE.txt`). Of sounddevice's
  PortAudio builds only the one she loads ships (none of the ASIO ones, built with Steinberg's SDK).
  `packaging/windows/voice-licenses.py` writes them, and stops the build if a wheel that carries
  someone else's binary isn't the version its notes were written for, on a native binary nobody
  named (DLL or exe), on a declared licence not accepted (whole words; a proprietary one beside it
  still stops it), and on a new or compiled GPL-family package.

**espeak-ng's source** (GPL-3.0 section 6(d): beside the object code). The voice installer carries
espeak-ng's DLL, so every release also carries `espeak-ng-<version>.tar.gz` (its tag),
`espeakng-loader-<version>-build.tar.gz` (the scripts that built the DLL) and
`sonic-<commit7>.tar.gz` (Bill Cox's sonic, Apache-2.0: espeak-ng's CMake fetches it and compiles
`sonic.c` into the DLL, so it is part of the DLL's source), fetched by `assemble`
and checked against SHA-256s pinned in release.yml; the notes say so, and `SOURCE.txt` links them.

CI checks each artifact holds all of them: the `installer` job on the Linux layout, release.yml on
the tarball and on its own voice installer, `windows-installer` and `windows-voice-installer` on
what Setup installed. ci.yml's `check` job generates the Linux crates' licences on every PR, so a new
licence fails there and not on the tag.

A pull request that touches the packaging runs the same builds and tests, and assembles the
release's files (`assemble`), and releases nothing. **Run workflow** on release.yml is the dry
run: everything a tag does, onto a draft pre-release named `dryrun-v<version>` at that commit (its
notes are `## Unreleased` until the version has its own section). It never makes a `v*` tag and is
never published; delete it when you're done.

## How it is tested

- **CI** (`ci.yml`, job `installer`):
  - shellcheck on `install.sh`, PSScriptAnalyzer on `install.ps1`;
  - `installer/test_parity.py`: both scripts take the same flags, run the same steps in the same
    order, read keys the manifest has, and `install.ps1` keeps nothing in the caller's scope;
  - `installer/test-units.sh`: her start at the end, the pinned Synaps and the daemon's protocol,
    her app-menu entry (one that isn't hers backed up first), which desktop is Hyprland (its
    lock's PID a Hyprland by name, and its display; no lock, a PID reused, a signature or desktop
    variable left over isn't), the GNOME shortcut (a stand-in `gsettings`, as GLib lists an
    extension's own schemas: never on `--yes` or without a terminal, your list kept, a write it says
    yes to and doesn't keep caught) and Hyprland's Super+Alt+C, and `--uninstall` taking them away
    (not an entry that isn't hers any more; what it can't take out stays in the receipt, with her
    prefixes);
  - `install.sh --dry-run` several ways (one fetched from GitHub as `curl | bash` gets it, one with a
    `--prefix` with spaces, one from `CAPRI_INSTALL_FLAGS`), checked to change nothing; as root,
    refused without `--allow-root` and always with someone else's `HOME`.
- **CI** (`windows-installer`): `installer/test-units.ps1` checks the path guard (a folder removed
  whole only by name), the recorded folders and the receipt's whole-line dedup on Windows paths; `installer/test-windows.ps1` runs `install.ps1` end to end with the
  installer that job built:
  - dry runs that change nothing, fetched as a scriptblock and as the real `irm | iex` (after which
    the session's preferences, variables and functions are as they were, `CAPRI_INSTALL_FLAGS` is
    cleared, and "remove her" is the piped form);
  - `--yes --no-autostart`;
  - `synaps --version`, her `--check`, the daemon;
  - a rerun, then `--uninstall` with nothing of hers left;
  - a `--prefix` with spaces over a pack you made (moved aside, kept by `--uninstall`), with a
    `Run\synaps-daemon` of yours (left as it is, still there after `--uninstall`);
  - `--purge` (her files go, another pack stays).
- **Containers** (podman/docker on a Linux box, unprivileged, private cgroup namespace, nothing
  mounted): Debian, Ubuntu, Arch, Fedora. Each one runs a dry run (nothing changed), the full
  `--yes --no-autostart` install, `synaps --version` and `synaps-avatar --check`, a rerun, and
  `--uninstall` (nothing of hers left). On top of that:
  - one source build, then an upgrade from a newer commit;
  - i3, Hyprland and GNOME stand-ins for steps 5 and 6;
  - a `--voice --watch` run;
  - `--prefix "/tmp/x/My Apps"`, with a decoy `/tmp/x/My` beside it (untouched by `--uninstall`);
  - a pack you made, moved aside and kept; `--purge` keeping another pack;
  - root: refused without `--allow-root`, refused with someone else's `HOME`;
  - Synaps' installer: refused without a terminal or `--yes` (nothing of Synaps installed, no shell
    file touched), refused when answered `n`, and with `--yes` the files it named are the ones it
    edited;
  - the brain step under a terminal with `--yes` (the picker isn't opened), after "start her";
  - the brain step against a stand-in `synaps` and her `--providers`, and stand-in Ollama and LM
    Studio servers: later with no terminal; `--brain` with an account (its login, then her model
    its default; a stand-in that waits on its stdin once signed in, as 0.12's does, ends at once,
    and Ctrl-C mid sign-in leaves no synaps and no holder of its stdin), a key (on synaps' stdin,
    never in its arguments or the output), a local model
    (Ollama's leaves Synaps' config alone; LM Studio's sets `provider.local.url` and reloads the
    daemon), one not running and a name she doesn't know (later, said); the picker (a local
    model, an account, Enter is later); a login already (nothing asked); `--dry-run`;
  - `--uninstall` naming the removal recipe only for a Synaps it installed;
  - `--uninstall --purge` with your own pack at `packs/capri` (kept); a receipt with a `dir` entry
    that isn't one of hers (left alone); a relative `--prefix` with `..` in it;
  - a stand-in `systemctl`: a unit rewritten by you is neither disabled nor removed.

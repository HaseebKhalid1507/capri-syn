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
command, then asks; `--yes` answers for it. x86_64, glibc 2.35 or newer for the prebuilt binary (older: `--from-source`). Tested on
Debian 12, Ubuntu 24.04, Arch and Fedora (see below). openSUSE (zypper) is wired, not tested.

## Windows

```powershell
irm https://raw.githubusercontent.com/HaseebKhalid1507/capri-syn/main/install.ps1 | iex
# with flags (iex can't pass them):
& ([scriptblock]::Create((irm https://raw.githubusercontent.com/HaseebKhalid1507/capri-syn/main/install.ps1))) --voice --yes
# or from the environment (read when given no flags, said in the banner, then cleared):
$env:CAPRI_INSTALL_FLAGS = '--voice --yes'; irm https://raw.githubusercontent.com/HaseebKhalid1507/capri-syn/main/install.ps1 | iex
```

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
its finish page offers the Claude sign-in: [docs/WINDOWS.md](WINDOWS.md#the-installer).

## The steps

| # | step | Linux | Windows |
|---|---|---|---|
| 1 | **Synaps** | When `synaps` is missing or older than `synaps_min` (0.11.0: her client speaks protocol 3), it says what Synaps is (her brain: an open-source agent runtime, yours to keep or remove) and which of your shell files its installer will add a PATH line to (`. "$HOME/.cargo/env"` in `~/.profile`, the bash rc files you have, a zsh one, fish's `conf.d`; none when `~/.cargo/bin` is on your PATH already), then **asks** (`--yes` is the consent; declined, it stops and prints how to install it yourself with `SYNAPS_NO_MODIFY_PATH=1`). Then it runs SynapsCLI's own `synaps-installer.sh` from its latest release, checked against GitHub's sha256 of that asset. If the API doesn't answer, it uses the manifest's pinned release and hash. | `synaps-installer.ps1` of the release the manifest pins (`synaps_pin`, one she was run against), checked against its pinned SHA-256; never the latest (a daemon on another protocol fails the daemon step). The edit it names is `~\.cargo\bin` on your user `Path`. |
| 2 | **Her binary** | The release tarball (`synaps-avatar-x86_64-unknown-linux-gnu.tar.gz`), checked against its `.sha256`. With no release (today), or with `--from-source`: rustup if missing (`rustup-init` checked against its sha256, `--no-modify-path`), the toolchain `rust-toolchain.toml` pins, and `cargo build --release --locked`, in `~/.cache/synaps-avatar/installer`. Before that it prints the distro's packages (apt, pacman, dnf or zypper), the `-dev` libraries for a source build and the runtime ones for a binary, then asks. It checks for a Vulkan driver and only says which one to install. | Her installer (`capri-syn-setup-x86_64.exe`), checked against its `.sha256`, run unattended (`/SYNAPS=skip /PACK=skip`: those are this script's steps). Without one: rustup, Visual Studio's C++ build tools (winget, asks), and a windowed build. |
| 3 | **Her art pack** | `tools/make-pack`: CapriCore v1.0.4 plus lachy's desktop art at their pinned commits, every file checked against `tools/*.sha256`, built with Python and fonttools in a venv of its own (never system-wide). It prints lachy's licence and Required Notice lines. A pack built from the same inputs is left alone. Mochi's art is **not** fetched: `--with-local-art` adds it, for local personal use only. | The same make-pack and pins, run by `packaging/windows/steps.ps1` (her installer's "Download her art" runs the same script) in CPython's embeddable package and pip, pinned and SHA-256 checked, in `%LOCALAPPDATA%\synaps-avatar\installer`: no Python of yours needed or touched. |
| 4 | **Daemon and her session** | When systemd's user manager runs, it writes and enables a `synaps-daemon.service` user unit; a unit of your own is kept, and enabled only if you agree. With no user manager, it starts the daemon now, and her autostart line starts it at login. It checks the daemon speaks protocol 3. Her session `capri` is her own, as before (attached if live, else resumed, else made). | It starts `synaps daemon --detach` now, plus a Run value (`synaps-daemon`) for sign-in. A `synaps-daemon` value of your own is left as it is and never recorded. |
| 5 | **Summon key** | i3, sway or Hyprland: shows the binding (`$mod+c`: press and release, so a hold talks). With your consent (`--yes`) it appends it between marker comments, after a timestamped backup. The key is named as your config sets `$mod` (Super for Mod4, Alt for Mod1). One that is already taken is reported, not overridden. On any other desktop it tells you what to bind (`avatarctl talk tap`). | Win+Alt+C: she registers it herself. |
| 6 | **Autostart** (default on) | i3 and sway get an `exec` line, Hyprland an `exec-once`, others an XDG autostart `.desktop`. Each one starts the daemon first if needed. | A Run value (`synaps-avatar`). |
| 7 | **`--voice`, `--watch`** | `--voice` runs `tools/capri-voice-setup` (its own venv, ~320 MB of models) and installs `capri-voice.service`. `--watch` installs `capri-watch.service`, enabled only with `--yes`. | `--voice` uses her voice installer (or builds the same bundle without a release) and fetches the models. `--watch` is Linux-only. |

At the end comes an `installed:` summary: what is where, how the daemon starts, how to summon her,
how to start her, and how to remove her (as you ran it: the checkout's script, or the piped form).

When Synaps has no model yet (no login in `~/.synaps-cli/auth.json`, no `local/` model in its config
or her `[session]`), one line says so: signing in is hers to walk you through, not a picker in the
installer. Until Synaps has a model her tag reads NO BRAIN YET, and her answer to your first message
ends on the command (`synaps login --provider claude` for Claude). The daemon picks a login up on her
next turn, with no restart: Synaps reads `auth.json` per request. When Synaps was already there with
a model, one line says her session is `capri`.

Then it starts her, `--no-autostart` or not (that flag is about login), the way autostart will
(Linux: `env SA_BACKEND=x11` off Hyprland and i3, so GNOME and KDE on Wayland run her through
XWayland), from your home folder, in a session of her own so closing the terminal leaves her be, and
waits up to 20 s for her first frame (`~/.local/state/synaps-avatar/launch.log`, the run before's
kept as `launch.old.log`; Windows: her `synaps-avatar.log`, which she starts over past 4 MB).

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

## Flags

| flag | |
|---|---|
| `--yes` | don't ask: Synaps' installer and its PATH edits, sudo for packages, edits to the window manager's config, enabling capri-watch (Windows: winget). It never opens `synaps login` |
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
- it removes the folders it made (when empty), the prefix's included.

It removes only what is inside her folders (each `--prefix` she went to, and her data, config and
cache folders): never `/` or your home, nothing with a newline or `..` in it. A folder removed
whole must be one it makes (her pack, `lib/synaps-avatar`, the installer's cache, the voice
folders). A unit is disabled and removed only while it still carries the header it wrote. An entry
outside those is reported and left alone.

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
- `~/.cache/synaps-avatar/`: the source, the build and the make-pack venv (reused by reruns);
- `~/.config/systemd/user/`: the units.

Windows: her installer's folder, her pack under `%LOCALAPPDATA%\synaps-avatar`, and the Run values.

## Releases

`.github/workflows/release.yml` runs on a `v*` tag. The tag must match `Cargo.toml`'s version. It
builds:
- the Linux tarball, on ubuntu-22.04 for glibc 2.35: her binary, avatarctl, the tools and units,
  and `install.sh`;
- her Windows installer and her voice installer.

Each asset gets a `.sha256`, is smoke-tested through the installers above, and is attached to that
tag's release. If there is no release yet, it creates a **draft**: publishing is a person's call,
and `releases/latest` (what the installers read) skips drafts. Until a release exists, both
installers build from source.

A pull request that touches the packaging runs the same builds and tests, and releases nothing.

## How it is tested

- **CI** (`ci.yml`, job `installer`):
  - shellcheck on `install.sh`, PSScriptAnalyzer on `install.ps1`;
  - `installer/test_parity.py`: both scripts take the same flags, run the same steps in the same
    order, read keys the manifest has, and `install.ps1` keeps nothing in the caller's scope;
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
  - `--uninstall` naming the removal recipe only for a Synaps it installed;
  - `--uninstall --purge` with your own pack at `packs/capri` (kept); a receipt with a `dir` entry
    that isn't one of hers (left alone); a relative `--prefix` with `..` in it;
  - a stand-in `systemctl`: a unit rewritten by you is neither disabled nor removed.

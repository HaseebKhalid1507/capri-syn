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

It never runs `sudo` on its own. It prints the exact package command, then asks; `--yes` answers
for it. x86_64, glibc 2.35 or newer for the prebuilt binary (older: `--from-source`). Tested on
Debian 12, Ubuntu 24.04, Arch and Fedora (see below). openSUSE (zypper) is wired, not tested.

## Windows

```powershell
irm https://raw.githubusercontent.com/HaseebKhalid1507/capri-syn/main/install.ps1 | iex
# with flags (iex can't pass them):
& ([scriptblock]::Create((irm https://raw.githubusercontent.com/HaseebKhalid1507/capri-syn/main/install.ps1))) --voice --yes
```

Per user, no admin (except the C++ build tools a source build needs, behind a UAC prompt). Every
flag also has PowerShell's spelling: `-Yes`, `-DryRun`, `-NoAutostart`, and so on.

## The steps

| # | step | Linux | Windows |
|---|---|---|---|
| 1 | **Synaps** | When `synaps` is missing or older than `synaps_min` (0.11.0: her client speaks protocol 3), it runs SynapsCLI's own `synaps-installer.sh` from its latest release, checked against GitHub's sha256 of that asset. If the API doesn't answer, it uses the manifest's pinned release and hash. | The same with `synaps-installer.ps1`. |
| 2 | **Her binary** | The release tarball (`synaps-avatar-x86_64-unknown-linux-gnu.tar.gz`), checked against its `.sha256`. With no release (today), or with `--from-source`: rustup if missing (`rustup-init` checked against its sha256, `--no-modify-path`), the toolchain `rust-toolchain.toml` pins, and `cargo build --release --locked`, in `~/.cache/synaps-avatar/installer`. Before that it prints the distro's packages (apt, pacman, dnf or zypper), the `-dev` libraries for a source build and the runtime ones for a binary, then asks. It checks for a Vulkan driver and only says which one to install. | Her installer (`synaps-avatar-setup-x86_64.exe`), checked against its `.sha256`, run unattended. Without one: rustup, Visual Studio's C++ build tools (winget, asks), and a windowed build. |
| 3 | **Her art pack** | `tools/make-pack`: CapriCore v1.0.4 plus lachy's desktop art at their pinned commits, every file checked against `tools/*.sha256`, built with Python and fonttools in a venv of its own (never system-wide). It prints lachy's licence and Required Notice lines. A pack built from the same inputs is left alone. Mochi's art is **not** fetched: `--with-local-art` adds it, for local personal use only. | The same, with Python from winget when there's none (asks). |
| 4 | **Daemon and her session** | When systemd's user manager runs, it writes and enables a `synaps-daemon.service` user unit; a unit of your own is kept, and enabled only if you agree. With no user manager, it starts the daemon now, and her autostart line starts it at login. It checks the daemon speaks protocol 3. Her session `capri` is her own, as before (attached if live, else resumed, else made). | It starts `synaps daemon --detach` now, plus a Run value (`synaps-daemon`) for sign-in. |
| 5 | **Summon key** | i3, sway or Hyprland: shows the binding (`$mod+c`: press and release, so a hold talks). With your consent (`--yes`) it appends it between marker comments, after a timestamped backup. A Super+C that is already taken is reported, not overridden. On any other desktop it tells you what to bind (`avatarctl talk tap`). | Win+Alt+C: she registers it herself. |
| 6 | **Autostart** (default on) | i3 and sway get an `exec` line, Hyprland an `exec-once`, others an XDG autostart `.desktop`. Each one starts the daemon first if needed. | A Run value (`synaps-avatar`). |
| 7 | **`--voice`, `--watch`** | `--voice` runs `tools/capri-voice-setup` (its own venv, ~320 MB of models) and installs `capri-voice.service`. `--watch` installs `capri-watch.service`, enabled only with `--yes`. | `--voice` uses her voice installer (or builds the same bundle without a release) and fetches the models. `--watch` is Linux-only. |

At the end comes an `installed:` summary: what is where, how the daemon starts, how to summon her.

## Flags

| flag | |
|---|---|
| `--yes` | don't ask: sudo for packages, edits to the window manager's config, enabling capri-watch (Windows: winget) |
| `--dry-run` | print every action, touch nothing |
| `--uninstall` | remove what it installed; settings, state and her log stay |
| `--purge` | with `--uninstall`: remove those too |
| `--voice` | her voice too |
| `--watch` | capri-watch too (Linux) |
| `--no-autostart` | don't start her at login |
| `--from-source` | build even when a release has a binary |
| `--prefix DIR` | where `bin/` and `lib/synaps-avatar/` go (Linux, default `~/.local`); her folder (Windows, default `%LOCALAPPDATA%\Programs\synaps-avatar`) |
| `--with-local-art` | mochi's art too: **local personal use only**, never publish or share it |
| `--artifact FILE` | install this release asset instead of downloading one (tests, offline) |

## What it writes, and uninstalling

Every file, folder, unit, Run value and config block it adds is listed in a receipt
(`~/.local/share/synaps-avatar/install.receipt`; Windows: `%LOCALAPPDATA%\synaps-avatar\install.receipt`).
`--uninstall` walks it backwards:
- it stops her;
- it disables the units it enabled;
- it removes the marker blocks, keeps your config's backup, and leaves a symlinked config a symlink;
- it removes the folders it made (when empty).

Synaps stays, since her daemon may have other clients, and so does rustup if it installed it.

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
    order, and read keys the manifest has;
  - `install.sh --dry-run` four ways (one fetched from GitHub as `curl | bash` gets it), checked
    to change nothing.
- **CI** (`windows-installer`): `installer/test-windows.ps1` runs `install.ps1` end to end with the
  installer that job built:
  - a dry run (also fetched with `irm`) that changes nothing;
  - `--yes --no-autostart`;
  - `synaps --version`, her `--check`, the daemon;
  - a rerun, then `--uninstall` with nothing of hers left.
- **Containers** (podman/docker on a Linux box, unprivileged, private cgroup namespace, nothing
  mounted): Debian, Ubuntu, Arch, Fedora. Each one runs a dry run (nothing changed), the full
  `--yes --no-autostart` install, `synaps --version` and `synaps-avatar --check`, a rerun, and
  `--uninstall` (nothing of hers left). On top of that:
  - one source build, then an upgrade from a newer commit;
  - i3, Hyprland and GNOME stand-ins for steps 5 and 6;
  - a `--voice --watch` run.

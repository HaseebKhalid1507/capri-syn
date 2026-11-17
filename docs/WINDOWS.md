# She on Windows (in progress)

Windows 11, x86_64-pc-windows-msvc. Everything Windows-only is in `src/platform/windows/`; GPUI's
Windows crate is vendored with small patches (`vendor/gpui-pre-windows/PATCHES.md`). CI builds,
tests and lints it on every push (the `windows` job). What CI can't see (a desktop, his input,
monitors) is checked in the capri-win VM: those items are marked **[VM] pending** below.

## Build and run

```powershell
cargo build --release --bin synaps-avatar
.\target\release\synaps-avatar.exe      # a console build: her log in the terminal
```

`capri-ctl` and the brain talk to her over the same local socket (AF_UNIX, Windows 10 1803+).

Her files (`platform::base_dir`; `$XDG_*_HOME` still wins, as on Linux):

| | where |
|---|---|
| settings | `%APPDATA%\synaps-avatar\settings.toml` (roams with him) |
| her pack (default `--pack`) | `%LOCALAPPDATA%\synaps-avatar\packs\capri` |
| state | `%LOCALAPPDATA%\synaps-avatar\state.json`, `sessions.json` |
| her log, without a console | `%LOCALAPPDATA%\synaps-avatar\synaps-avatar.log` (appended; over 4 MB at start, kept as `.old.log`) |

## The installer

`packaging/windows` (Inno Setup 6). CI's `windows-installer` job builds it on every PR and push to
main and uploads it as an artifact (`synaps-avatar-setup-<sha>`, kept 14 days): never released or
published. Locally, on a machine with Rust, Inno Setup and ImageMagick: `.\packaging\windows\build.ps1`.

- **Per user, no admin**: she goes to `%LOCALAPPDATA%\Programs\synaps-avatar`.
- **Her exe is the windowed build** (`cargo rustc … /SUBSYSTEM:WINDOWS`, her binary only): no
  console window from the Start menu or at sign-in. Without a console she logs to the file above.
  `--check` from a terminal therefore prints nothing there: read the log, or use a cargo build.
- **Her pack is never in it.** The wizard's "Her pack" page (unattended: `/PACK="C:\dir"`) takes a
  pack folder on his disk (one with `sprites.json`) and copies it to the default pack location; a
  folder that isn't a pack is refused. Empty: the location is made for him to fill. Nothing is
  downloaded. A new pack replaces the old one whole.
- **Her AUMID** `Synaps.Avatar`: registered under `HKCU\Software\Classes\AppUserModelId` with
  `DisplayName` and `IconUri`, and set on the Start-menu shortcut, so toasts show under her name
  and icon. The icon is a mark in her colours (her red bolt on black), drawn at build time: no art.
- **Autostart, opt-in** (a task, unchecked): the `HKCU\…\Run` value.
- **Uninstall** (Settings → Apps, or `unins000.exe /VERYSILENT`): stops her, then removes her exe,
  the shortcut, the AUMID key, the Run value, the pack it copied, her log and state, and her folders
  if empty. His settings (`%APPDATA%\synaps-avatar`) and any pack he put there himself stay.
- **Upgrades** close her (Restart Manager) and don't restart her.
- CI installs, checks and uninstalls it unattended (`test-install.ps1`, a stand-in pack, no art):
  the files, keys, shortcut AUMID, the windowed exe, her log and default pack path, and that
  nothing is left after.

Unattended: `synaps-avatar-setup-….exe /VERYSILENT /SUPPRESSMSGBOXES [/PACK="C:\packs\capri"] [/TASKS=autostart]`.

## Her voice: a second installer

`synaps-avatar-voice-setup-….exe` (`packaging/windows/synaps-avatar-voice.iss`, built by
`build-voice.ps1`; CI's `windows-voice-installer` job, an artifact). **Its own download, not a
component of hers:** an Inno component sits inside `setup.exe` even unchecked, so every download of
her would carry the 143 MB bundle (~45 MB compressed). As a second installer, her default download
stays ~9 MB (CI fails hers past 20 MB), and only he who wants voice gets the rest.

- **Per user, no admin**: the bundle goes to `voice\` in her install folder (read from her installer's
  record, else its default), where she looks for it.
- **The models are never in it.** The "models" task (checked) runs `voice-models.ps1`: Whisper small.en and
  base.en, Kokoro int8 and its voices, Silero, the wake word's two feature models (~320 MB, each SHA-256
  pinned; "hey capri" itself comes with the bundle) into `%LOCALAPPDATA%\capri-voice\models`. A cut
  download resumes (`curl.exe -C -`); a wrong hash is deleted and the run fails. A Start-menu entry,
  "synaps-avatar voice models", runs it again.
- She starts the sidecar once the models are there (her log says so until then).
- **Uninstall** stops the sidecar (only the `python.exe` in that folder), removes the bundle, Python's
  caches, and `%LOCALAPPDATA%\capri-voice` (models, log, link, media state).
- CI installs, checks and uninstalls it unattended without the models (`test-voice-install.ps1`: the
  files, the selftest run by the installed interpreter, the folder beside her in both cases, nothing
  left), and runs the model fetch on its small files (`test-voice-models.ps1`: resume, a bad hash, a
  kept file).

Unattended: `synaps-avatar-voice-setup-….exe /VERYSILENT /SUPPRESSMSGBOXES [/MERGETASKS="!models"]`.

## install.ps1: one script (Synaps too)

`irm https://raw.githubusercontent.com/HaseebKhalid1507/capri-syn/main/install.ps1 | iex` runs
`install.sh`'s seven steps the Windows way (`docs/INSTALL.md`):
1. Synaps through its own `synaps-installer.ps1`, sha256-checked;
2. her installer from the latest release, or a windowed source build (rustup, and Visual Studio's
   C++ build tools via winget behind a UAC prompt, asked first);
3. her pack through `tools/make-pack` in a venv of its own (Python from winget if there's none,
   asked first);
4. `synaps daemon --detach` now, and a Run value for sign-in;
5. Win+Alt+C (she registers it herself);
6. a Run value for her (default on);
7. `--voice`: her voice installer and the models.

Per user. A receipt in `%LOCALAPPDATA%\synaps-avatar` drives `--uninstall`.

CI (`windows-installer`, every PR) runs it end to end on windows-latest with the installer that
job built (`installer/test-windows.ps1`):
- a dry run (also fetched with `irm`) that changes nothing;
- `--yes --no-autostart`;
- `synaps --version`, her `--check`, the daemon and its Run value;
- a rerun (up to date);
- `--uninstall`, which leaves nothing of hers.

What CI can't show is item 15 below.

## What she does here

| | how | status |
|---|---|---|
| her body, the Line, the reader, the intro | GPUI pop-ups made bare topmost tool windows: no taskbar button, not in Alt+Tab, no DWM frame or rounding | P0 [VM] |
| click-through | layered + `WS_EX_TRANSPARENT`, toggled by a 10 ms pointer poller over her input region | P0 [VM] |
| perching | `EnumWindows`, DWM frame bounds, WinEvent hooks for moves, shows, hides, minimise, cloak, foreground | P0 [VM] |
| virtual desktops | windows on another desktop are cloaked (left out); `IVirtualDesktopManager` confirms; a switch (the shell's registry value) is `Hint::Desktop` | **[VM] pending** |
| monitors | `EnumDisplayMonitors`; `WM_DISPLAYCHANGE`, `WM_DPICHANGED` and a work-area change (taskbar moved) are `Hint::Layout` | **[VM] pending** |
| DPI | per-monitor aware (v2), set before GPUI starts. Her windows keep the primary monitor's scale everywhere (PW-5): her art is laid out at one scale | **[VM] pending** (two monitors at different scales) |
| effect layers | the bolt and anything reaching outside her window: a plain Win32 window per layer, solid colour, shaped by a region, click-through, never activated (`fx.rs`). The rest is painted in her window | **[VM] pending** (visual) |
| her keys | `RegisterHotKey`: **Win+Alt+C** summons (a tap), **Win+Alt+Ctrl+C** toggles talk. Win alone is the shell's | **[VM] pending** |
| the Line's keyboard | see below | **[VM] pending** |
| music | SMTC (the media flyout's sessions): playing or quiet, and whether every playing session is a video | **[VM] pending** |
| notifications | toasts, see below | **[VM] pending** |
| lock, sleep, idle | WTS session notifications, power broadcasts, `GetLastInputInfo` | P0 [VM] |
| voice: the link | her end of the sidecar's link: loopback TCP after a two-token handshake (`endpoint.json` in `%LOCALAPPDATA%\capri-voice`); Linux keeps the Unix socket | CI (both ends, Rust and Python) |
| voice: the sidecar | `tools/capri_voice_win.py`: PortAudio (WASAPI) capture and playback, SMTC pause, session-volume duck with Linux's crash repair, lock, calls (the mic privacy store), DND. The voice installer (below) puts its bundle (embeddable CPython 3.13 + wheels) beside her: `docs/design/windows-voice.md` | CI; **[VM] pending** (live audio: item 13) |
| voice: she runs it | while `[voice] enabled`: `python.exe capri-voice` from `voice\` beside her exe (or `CAPRI_VOICE_BUNDLE`), no console window, in a job object that kills it with her (however she ends); restarted after 1, 2, 4 … 60 s; its log `%LOCALAPPDATA%\capri-voice\capri-voice.log`. Not before the models are there. `CAPRI_VOICE_SPAWN=0`: he runs it himself | CI (job, backoff, bundle lookup); **[VM] pending** (item 14) |
| voice: connecting | on a thread (a stale endpoint can't freeze her: up to ~2 s); Linux connects inline, as before | CI (a silent listener) |
| hold-to-talk | opt-in, `[voice] hold_to_talk = true`: the summon key's press is `talk press`; her key thread then reads the key every 25 ms (`GetAsyncKeyState`, as OBS does) and its release is `talk release`. No keyboard hook. Off: the summon key is a tap, as before | CI (the state machine); **[VM] pending** (item 14) |

## The Line's keyboard

Windows hands a process the foreground only right after the user's own input to it. So
(`docs/design/windows-line-keyboard.md`):

- her key and a click on her take the keyboard at once;
- any other opening (a summon from the brain, `capri-ctl`, voice) shows the Line **without** the
  keyboard, and her tag says `WIN+ALT+C TO TYPE`; his key or a click on the Line then takes it;
- she never takes the keyboard from the window he works in, never flashes, never injects input.

## Toasts

Without the installer she registers her AppUserModelID herself, once, under
`HKCU\Software\Classes\AppUserModelId\Synaps.Avatar` (`DisplayName`): no admin, no shortcut.
Windows then shows her toasts in the Action Center under that name, with the image as the logo.
The installer adds the icon (`IconUri`) and the shortcut's AUMID. The id has to stay
`Synaps.Avatar` (`notifications.rs` and `synaps-avatar.iss` share it).

**A click on a toast does nothing**, as on Linux (her notifications there carry no action). Making
it open the Line was left out: it needs a COM activator, a `ToastActivatorCLSID` on the shortcut
(Inno: `AppUserModelToastActivatorCLSID`), the CLSID's `LocalServer32` under HKCU, and in her process
a class factory and `INotificationActivationCallback` registered at start (`CoRegisterClassObject`),
plus a cold start when she isn't running. About half a day plus a VM check; not cheap without a
desktop to try it on.

## [VM] pending (one VM day)

1. PW-1/2/3 (from P1a): no frame; a click on her doesn't take focus; no stale frame or extra CPU while
   she's dragged.
2. PW-4: no stray Alt (menu key tips) when the Line opens.
3. PW-5 + DPI: two monitors at 100 % and 150 %: she keeps her size crossing over; no blur.
4. Hotplug: add or remove a monitor, move the taskbar: she re-reads (`Hint::Layout` in the log).
5. Her keys: both register (or log the taken chord); a press arrives; the summon takes the keyboard.
6. The Line from a summon or `capri-ctl`: opens waiting, her tag shows `WIN+ALT+C TO TYPE`, the
   keyboard stays in his window; his key or a click on the Line then takes it; closing gives it back.
7. The contract suite (`SA_CONTRACT_WINDOWS=1 … --ignored contract_windows`, interactive session):
   `keyboard` through his click (`SendInput`), `fx` with layer windows, and the rest as in P0.
8. Virtual desktops: a perch target on another desktop is left out; a switch re-reads at once.
9. Effect layers: the bolt shows outside her window, click-through, under/over her as asked.
10. SMTC: Spotify or a browser playing → `smtc playing (…)` in the log and LISTENING; a video →
    `ok listening (video)`; a Discord call (ignored) doesn't count.
11. Toasts: one shows under "synaps-avatar" with her face; the registry key is there.
12. The installer, interactively: the pack page (Browse, a non-pack refused, empty allowed); the
    Start-menu shortcut starts her with no console window; a toast after install shows her icon;
    autostart starts her at sign-in; uninstall from Settings → Apps leaves nothing but his settings.
13. Voice (his machine for the audio; the VM for the rest), the [VM] column of
    `docs/design/windows-voice.md` → V1a:
    - the bundle on a clean Windows (does onnxruntime want the VC++ runtime?);
    - mic and speaker end to end: hold, toggle, hands-free, the wake word, a hush;
    - unplugging a headset mid-hold;
    - the duck by ear, and pause with Spotify, Edge and Media Player;
    - a call (Teams, Discord, the Sound Recorder) closing the hands-off mic;
    - Win+L mid-hold;
    - whether Windows 11's Do not disturb shows in `SHQueryUserNotificationState`;
    - headphones told from speakers;
    - Whisper and Kokoro speed on his CPU.
14. Voice, her side (V1b):
    - the voice installer interactively, the models task's window, then the sidecar started by her
      (`voice: sidecar started` in her log), and gone when she is killed (Task Manager);
    - voice off in settings stops it; on starts it again;
    - an uninstall of voice while she runs (she restarts it after 1 s: the uninstaller may race her);
    - `hold_to_talk = true`: holding Win+Alt+C talks and the release sends; a quick tap opens the Line;
      Win+L mid-hold ends it; no stuck hold after a release in another app's window;
    - no firewall prompt for the sidecar's loopback port.
15. install.ps1 on a clean Windows 11, interactively (CI covers the unattended run):
    - `irm … | iex` in Windows PowerShell 5.1, with no Python: the winget prompt, Python installed
      per user, the pack built (or "open a new terminal" when PATH isn't refreshed);
    - `--from-source` with no build tools: winget's Visual Studio Build Tools behind UAC (~3 GB),
      then the build;
    - after a sign-out and back in: the Run values start the daemon and her, and she finds her
      session. Whether `synaps.exe daemon --detach` (a console program) flashes a console window;
      if it does, a hidden start (`conhost --headless` or a tiny launcher) is the fix;
    - `--voice` against a real release (the voice installer, the models);
    - SmartScreen and Defender on the unsigned installers it downloads and runs;
    - `--uninstall` while she runs.

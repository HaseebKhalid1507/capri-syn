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
| voice: the sidecar | `tools/capri_voice_win.py`: PortAudio (WASAPI) capture and playback, SMTC pause, session-volume duck with Linux's crash repair, lock, calls (the mic privacy store), DND. A bundle (embeddable CPython 3.13 + wheels, `packaging/windows/voice-bundle.ps1`) runs it with no Python installed. Not started by her yet, not in the installer: `docs/design/windows-voice.md` | CI; **[VM] pending** (live audio: item 13) |
| hold-to-talk | not yet (tap and toggle work); options in `docs/design/windows-voice.md` §6 | — |

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

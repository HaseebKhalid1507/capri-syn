# She on Windows (in progress)

Windows 11, x86_64-pc-windows-msvc. Everything Windows-only is in `src/platform/windows/`; GPUI's
Windows crate is vendored with small patches (`vendor/gpui-pre-windows/PATCHES.md`). CI builds,
tests and lints it on every push (the `windows` job). What CI can't see (a desktop, his input,
monitors) is checked in the capri-win VM: those items are marked **[VM] pending** below.

## Build and run

```powershell
cargo build --release --bin synaps-avatar
.\target\release\synaps-avatar.exe      # packs as on Linux (~/.config/synaps-avatar, SA_PACK, ...)
```

`capri-ctl` and the brain talk to her over the same local socket (AF_UNIX, Windows 10 1803+).

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
| hold-to-talk, voice | not yet (no key-up without a low-level hook, which she won't install; voice isn't ported) | — |

## The Line's keyboard

Windows hands a process the foreground only right after the user's own input to it. So
(`docs/design/windows-line-keyboard.md`):

- her key and a click on her take the keyboard at once;
- any other opening (a summon from the brain, `capri-ctl`, voice) shows the Line **without** the
  keyboard, and her tag says `WIN+ALT+C TO TYPE`; his key or a click on the Line then takes it;
- she never takes the keyboard from the window he works in, never flashes, never injects input.

## Toasts and what packaging must provide

Unpackaged, she registers her AppUserModelID herself, once, under
`HKCU\Software\Classes\AppUserModelId\Synaps.Avatar` (`DisplayName`): no admin, no Start-menu
shortcut. Windows then shows her toasts in the Action Center under that name, with the image as
the logo. A click on a toast does nothing yet.

A package (MSIX, or an installer) must provide:

- **the AUMID**: MSIX gives one from the manifest; an installer writes a Start-menu shortcut with
  `System.AppUserModel.ID = Synaps.Avatar` (or keeps the registry entry above). Either way the id
  has to stay `Synaps.Avatar`, or `notifications.rs` changes with it;
- **an icon** for the toast header (`IconUri` under the same key, or the shortcut's/manifest's);
- **toast activation**, if a click should open the Line: a COM activator (`ToastActivatorCLSID`
  on the shortcut, or the manifest's `com:Extension`) and a handler in her process;
- **her autostart**, if wanted: a Run key or a startup task (nothing registers one now).

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

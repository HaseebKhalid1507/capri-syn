# The Line's keyboard on Windows, under the foreground lock (design note, undecided)

**Status:** options only. Haseeb or Jawz picks. Nothing here is built yet.

## The problem

The Line takes the keyboard when it opens (a summon, a tap of the talk key, a click on her), and gives
it back to the window he was in when it closes (`Stage::grab_keyboard` / `release_keyboard`). On X11
that is `XSetInputFocus`, and on Hyprland it is the layer surface's keyboard mode. On Windows it is
`SetForegroundWindow`. Windows refuses that call unless the caller qualifies: it is the foreground
process, it **received the last input event**, it was started by the foreground process, the foreground
process allowed it (`AllowSetForegroundWindow`), or no window is in the foreground (the lock timeout).
Otherwise the taskbar button flashes and nothing moves. In P0 [VM], the `keyboard` contract scenario
failed this way: `focus None after the grab`.

Which openings qualify:

| opening | who received the last input | qualifies today? |
|---|---|---|
| a registered hotkey (Win+Alt+C, `talk tap`) | her process (`WM_HOTKEY` is input to the thread that registered it) | **likely yes** (docs: "received the last input event"); [VM] check pending |
| a click on her body | her process (her window took the click, with PW-2: without activation) | likely yes, but PW-2's `MA_NOACTIVATE` means she must activate the Line herself |
| a summon from the brain (a prompt, a finished turn) | his app | **no** |
| `capri-ctl` from a terminal / a voice turn from the sidecar | the terminal, or nobody | **no** |
| giving the keyboard back on close | her process (he typed in the Line) | yes |

The hard cases are the ones without his input: she opens the Line on her own, or another process opens it.

## Options

### A. A hotkey-initiated focus only (the strict reading)
The Line takes the keyboard only when he asked for it with her key or a click. Every other opening shows
the Line **without** the keyboard, with a hint on it (`WIN+ALT+C TO TYPE`), and his next press of her key
focuses it.
- *For:* no tricks. It is the rule Windows wants, and nothing breaks on a Windows update.
- *Risk:* it changes the UX on Windows only. A summon no longer means "type now", so a prompt needs one
  more key press. Low technical risk. Needs a VM check that `WM_HOTKEY` really grants the right.

### B. `AttachThreadInput` to the foreground thread, then `SetForegroundWindow`
Attach her UI thread's input queue to the foreground window's thread, set the foreground, then detach.
- *For:* well known, works on every Windows from XP to 11 today, and it covers the summon case.
- *Risk:* it is the documented way around the lock, and Microsoft has narrowed it before. It joins
  the two threads' input states for a moment (key state and focus are shared, and a hung foreground app
  can hang her). Some games' anti-cheat (the same ones that dislike low-level hooks) flag cross-process
  input attachment. Steal-focus in the middle of his typing is exactly what the lock exists to stop, so
  it needs a rule: only when he has been idle for N s, or never during a game or fullscreen.

### C. The Alt-key trick (`SendInput` a lone Alt press and release, then `SetForegroundWindow`)
A synthetic keypress makes her the process that "received the last input", so the call succeeds.
- *For:* a few lines, and it covers every opening.
- *Risk:* the highest. It injects input into his session: a lone Alt opens or toggles menu bars in
  some apps (Office's key tips, Explorer's menus), and it can land in a game. Anti-cheat and accessibility
  tools see synthetic input. It is an undocumented side effect that Windows can close at any time. Not
  recommended except as a last resort behind a setting.

### D. A separate focusable Line window that asks politely
Keep the body's surfaces non-activating. The Line is an ordinary activatable window: `ShowWindow` with
`SW_SHOW`, and if Windows refuses the foreground it **flashes** (`FlashWindowEx`), then takes the keyboard
on his first click or key. Combine with A for her key: that path focuses immediately.
- *For:* honest. It degrades the way every Windows app's does, and the code is close to what GPUI already
  does for normal windows.
- *Risk:* the Line would need a taskbar button for the flash to show, which breaks "not in Alt+Tab or
  the taskbar". A flash on a tool window is invisible, so it needs another cue: her tag, or a toast. Two
  activation models (body vs Line) to keep straight in PW-2.

### Also considered
- `LockSetForegroundWindow` / `SPI_SETFOREGROUNDLOCKTIMEOUT = 0`: a system-wide setting change, so not
  ours to make.
- A low-level keyboard hook to read keys without focus: the anti-cheat problem again, and the reason
  hold-to-talk isn't built on Windows. Rejected.
- UI Automation `SetFocus`: it ends in the same foreground rules for another process's top-level window.

## Suggestion, if one is wanted

A + D: her key and her body take the keyboard at once (the input is his). Everything else opens the
Line unfocused, with a one-key hint, and never steals focus. B stays in reserve behind a setting if the
extra key press proves annoying. Whatever is picked, the `keyboard` contract scenario's Windows driver
must be updated to open the Line via a hotkey or a click, since the test process can't grab the
foreground either. Then run it in the VM.

**Size:** A + D is S–M (about 1–2 days with the VM checks); B is S plus its guard rules; C is S but not
advised.

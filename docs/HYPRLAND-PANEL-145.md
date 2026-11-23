# Hyprland panel height (#145)

## Cause (1.2.0)

Both screenshots show the **same Line window**, once on chat and once on Settings. It is
approximately 120 px high, with about 114 px inside its 3 px ring/frame inset on each edge.
This is the application's bootstrap size, not a 100% scaling error or a compositor minimum.

The path is:

1. `lineview::open_line_on` requests `LINE_H0 = 120` logical px in `WindowOptions`.
2. `Os::window_options` lets `LayerStage::dress` replace the normal window kind with a
   `capri-line` **overlay layer-shell** surface, anchored top-left, exclusive zone -1. Settings
   is a page of that same surface. There is no xdg-toplevel to tile, no app-id window rule to
   match, and no xdg min/max hint involved. GPUI sends these bounds as layer `set_size` before
   the initial empty commit; Hyprland's layer configure drives GPUI's viewport/buffer size.
3. `Line::render` lays out absolute, unconstrained content and defers `Ask::Height` when its
   measurement changes. `line_ask` clamps that height to the work area (and a 60 px floor),
   then `line_follow` calls `Os::place` with the new width and height.
4. **`LayerStage::place` discarded both dimensions (`_w`, `_h`) and only sent margins.**
   The core recorded the new size in `LineWin::at`, but neither `Window::resize` nor another
   layer `set_size` followed. There was no reason for Hyprland to configure a larger surface.
   X11's `place` sends width/height; Windows has its own sizing path.
5. The frame still clips children to the viewport. Chat's field and Settings' lower rows
   therefore exist below the visible 120 px surface. This looks like input/settings failing.

No trace from the reporter is available, so a particular configure sequence on their machine
has **not** been observed. The code explains both screenshots without a tiling or 0x0 configure.
GPUI treats a layer configure with either dimension zero as no size change; a positive configure
resizes its buffer. Initial configure commonly precedes the content measurement, which is why
sending the *post-layout* size matters. A hard-coded initial height or min-size hint would mask
rather than fix the missing resize.

The bubble is drawn inside the body's host, not this separately measured panel. Body hosts
are created at their output size; effect hosts start at 1×1 and `host::fx_hosts_sync` explicitly
calls `Window::resize` when their bounds change. The reader estimates its height before opening.
Those paths do not rely on a Line measurement reaching `place` to escape a 120 px bootstrap.

## Change

Hyprland `place` now converts device dimensions to positive, rounded-up logical dimensions
and calls the new vendored `layer_ctl::set_bounds`. It sends size and margins in one existing
`Commit::WithFrame` transaction, including the idle commit fallback. The compositor's configure
still drives GPUI's resize callback and buffer, rather than resizing only the application's
bookkeeping. A late old configure is acknowledged/rendered normally, but its geometry echo
cannot replace the latest explicit requested size. Only callers of this new layer setter opt in.

No X11, Windows, xdg-toplevel/popup or other-compositor behavior changes. These panels stay
on Overlay, outside tiling. No installer/config change is needed: `install.sh` already confines
its summon binds and autostart lines to `block_put` markers, but a `windowrule` cannot size a
layer-shell surface and would not repair this bug. No user rule is installed.

## Verification

- Pure Rust tests: bootstrap → measured chat → Settings → shrink; 100%, 125%, 150%, 200%
  conversion; dimensions never become zero; late old/zero configure; normal geometry unchanged.
  Both pure modules can be tested with `rustc --edition 2024 --test` from a wrapper module.
  The app's normal Linux unit suite also includes the vendored configure tests.
- `platform::hypr::panel_test::contract_hypr_panels` opens the **real Line** and Settings through
  GPUI with a deterministic transcript and draft, without art, a daemon, login or audio.
  It exercises fresh chat, fresh Settings and both page swaps. It asserts the layer and GPUI
  viewport grow to the measured content (chat at least 180 px; Settings at least 300 px), the
  cog/input row or bottom CHAT control is inside the viewport, and the surface stays an overlay
  absent from the xdg client list. It checks again after a later frame. On the old `place`, the
  test must fail with `layer=120 viewport=120` despite a larger measured height.

Run the prebuilt app test binary on a provisioned, **non-personal** rig runner:

```sh
# Inside the existing bwrap rig; the command refuses a normal desktop session.
tools/wlrig/run.sh env SA_CONTRACT_HYPR=1 /absolute/path/to/synaps_avatar-TEST_HASH \
  --ignored --test-threads 1 --nocapture contract_hypr_panels
```

This change was prepared without Cargo. The local rig startup was attempted inside its sandbox;
`inner.sh` failed with `sway: command not found`, and `Hyprland` is absent too. No application was
run on a user's desktop. At base a31885e the checked-in `ci.yml` runs the compile/unit suite, not
`tools/wlrig`; the existing `contract_hypr` is ignored, and no repository self-hosted runner was
listed. `sandbox.sh` also requires an Intel render node at PCI 0000:00:02.0 and Intel Vulkan;
`up.sh` requires an SSH, seatless session. A stock GitHub-hosted Ubuntu runner cannot meet
those requirements. The new panel contract is compile-checked by normal CI but requires that
provisioned rig to execute. It has **not** been claimed as a successful live reproduction or
compositor run.

## Reporter confirmation if the rig remains unavailable

Use a build from this PR and provide, with both chat and Settings open in turn:

- `hyprctl version`, `hyprctl -j monitors`, `hyprctl -j layers`, `hyprctl -j clients`;
- `avatarctl state` (the `line` object suffices: redact transcript/draft-related data), and a
  screenshot showing the full panel including the field or last Settings row;
- whether `SA_BACKEND` is set and whether the process has `WAYLAND_DISPLAY` and
  `HYPRLAND_INSTANCE_SIGNATURE` (do not send the rest of the environment).

If it is still clipped, a short `WAYLAND_DEBUG=client` startup/open/close trace identifies the
`zwlr_layer_surface_v1.set_size`, `configure` and `ack_configure` sequence. Use no sensitive draft
or conversation and redact other protocol data. We need the requested/configured sizes and
ordering, not credentials or the contents of their config. Their existing window/layer rules
are useful only if these sizes diverge or they explicitly force the XWayland backend.

# tools/wlrig/sandbox.sh: the Wayland rig's sandbox (BRIEF-wayland §4), sourced by up.sh, run.sh, down.sh.
# Every rig process runs in `rig_bwrap`, which enforces §4.1 by construction:
#   1. bubblewrap, new pid/ipc/net/uts namespaces, dies with its parent; a fresh /dev holding only the
#      Intel render node (never a card node, never NVIDIA's render node or /dev/nvidia*, never /dev/uinput);
#      private /tmp and /run (no X sockets: :0 unreachable; no system bus, no his runtime dir).
#   2. no seat, no DRM backend, no systemd hooks: seatd pointed at nothing, AQ_DRM_DEVICES=/dev/null,
#      HYPRLAND_NO_SD_*; a private session bus; Mesa EGL and the Intel Vulkan driver only (NVIDIA's
#      libraries never load).
#   4. the rig's own config, HOME, XDG dirs and runtime dir under $RIG.
RIG=${RIG:-/tmp/capri-wl-rig}
WLRIG=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
die() { echo "wlrig: $*" >&2; exit 1; }
NODE=$(readlink -f /dev/dri/by-path/pci-0000:00:02.0-render 2>/dev/null || true)
[[ "$NODE" =~ ^/dev/dri/renderD[0-9]+$ ]] || die "no render node at pci-0000:00:02.0"
[ "$(cat "/sys/class/drm/${NODE##*/}/device/vendor" 2>/dev/null)" = 0x8086 ] || die "$NODE is not Intel's"
RUNDIR=/run/user/$(id -u)

rig_bwrap() {
  [ -d "$RIG/run" ] || die "no rig at $RIG (up.sh)"
  bwrap --ro-bind / / \
    --dev /dev --tmpfs /dev/shm --dev-bind "$NODE" "$NODE" --proc /proc \
    --tmpfs /tmp --tmpfs /run --bind "$RIG/run" "$RUNDIR" --bind "$RIG" "$RIG" \
    --unshare-pid --unshare-ipc --unshare-net --unshare-uts --die-with-parent --new-session \
    --clearenv \
    --setenv PATH "$RIG/bin:/usr/local/bin:/usr/bin:/bin" --setenv USER "$(id -un)" --setenv LANG C.UTF-8 --setenv TERM xterm-256color \
    --setenv HOME "$RIG/home" --setenv XDG_CONFIG_HOME "$RIG/home/.config" --setenv XDG_STATE_HOME "$RIG/home/.local/state" \
    --setenv XDG_DATA_HOME "$RIG/home/.local/share" --setenv XDG_CACHE_HOME "$RIG/home/.cache" \
    --setenv XDG_RUNTIME_DIR "$RUNDIR" --setenv DBUS_SESSION_BUS_ADDRESS "unix:path=$RUNDIR/bus" \
    --setenv __EGL_VENDOR_LIBRARY_FILENAMES /usr/share/glvnd/egl_vendor.d/50_mesa.json \
    --setenv VK_DRIVER_FILES /usr/share/vulkan/icd.d/intel_icd.json \
    --setenv LIBSEAT_BACKEND seatd --setenv SEATD_SOCK /nonexistent --setenv AQ_DRM_DEVICES /dev/null \
    --setenv HYPRLAND_NO_SD_VARS 1 --setenv HYPRLAND_NO_SD_NOTIFY 1 --setenv HYPRLAND_NO_CRASHREPORTER 1 \
    --setenv RIG "$RIG" --setenv WLRIG "$WLRIG" --setenv NODE "$NODE" \
    "$@"
}

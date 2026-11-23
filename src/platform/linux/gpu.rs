//! Pick a render device before GPUI (and the Vulkan loader) starts.

use super::Backend;
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Gpu {
    pub id: String,
    pub vendor: String,
}

pub(super) struct Card {
    gpu: Gpu,
    boot: bool,
}

fn pci_id(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let text = text.trim();
    let id = u16::from_str_radix(
        text.strip_prefix("0x")
            .or_else(|| text.strip_prefix("0X"))
            .unwrap_or(text),
        16,
    )
    .ok()?;
    Some(format!("0x{id:04x}"))
}

fn pci_gpu(device: &Path) -> Option<Gpu> {
    Some(Gpu {
        id: pci_id(&device.join("device"))?,
        vendor: pci_id(&device.join("vendor"))?,
    })
}

/// Both primary and render nodes resolve to their PCI device through sysfs.
fn gpu_from_dev(sys: &Path, dev: libc::dev_t) -> Option<Gpu> {
    pci_gpu(&sys.join(format!(
        "dev/char/{}:{}/device",
        libc::major(dev),
        libc::minor(dev)
    )))
}

pub(super) fn cards() -> Vec<Card> {
    cards_in(Path::new("/sys/class/drm"))
}

fn cards_in(drm: &Path) -> Vec<Card> {
    let read = |p| {
        std::fs::read_to_string(p)
            .map(|s| s.trim().to_string())
            .unwrap_or_default()
    };
    std::fs::read_dir(drm)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !name.starts_with("card") || name.contains('-') {
                return None;
            }
            let device = entry.path().join("device");
            Some(Card {
                gpu: Gpu {
                    id: read(device.join("device")),
                    vendor: read(device.join("vendor")),
                },
                boot: read(device.join("boot_vga")) == "1",
            })
        })
        .collect()
}

/// Aquamarine's colon-separated priority list: only its first entry is the main device.
/// Use aliases without colons (e.g. gpu-nvidia), as required by AQ_DRM_DEVICES itself.
fn aq_first(value: &str) -> Option<&Path> {
    let first = Path::new(value.split(':').next()?.trim());
    first.is_absolute().then_some(first)
}

fn aq_gpu() -> Option<Gpu> {
    let value = std::env::var("AQ_DRM_DEVICES").ok()?;
    // metadata follows symlinks such as /dev/dri/gpu-nvidia.
    let meta = std::fs::metadata(aq_first(&value)?).ok()?;
    meta.file_type().is_char_device().then_some(())?;
    gpu_from_dev(Path::new("/sys"), meta.rdev())
}

/// Lazy probes: never ask Wayland on X11/XWayland, or AQ when feedback already resolved.
fn pick_in(
    backend: Backend,
    cards: &[Card],
    feedback: impl FnOnce() -> Option<Gpu>,
    aq: impl FnOnce() -> Option<Gpu>,
    screen: impl FnOnce() -> Option<Gpu>,
) -> Option<(Gpu, &'static str)> {
    match backend {
        Backend::Wayland => {
            if let Some(gpu) = feedback() {
                return Some((gpu, "Wayland linux-dmabuf main_device"));
            }
            if let Some(gpu) = aq() {
                return Some((gpu, "AQ_DRM_DEVICES first entry"));
            }
        }
        Backend::XWayland | Backend::XWaylandAuto => {
            if let Some(gpu) = screen() {
                return Some((gpu, "XWayland screen (DRI3)"));
            }
        }
        _ => {}
    }
    // Keep the measured X11 rule, including leaving a single-GPU machine alone.
    if cards.len() < 2 {
        return None;
    }
    cards
        .iter()
        .find(|c| c.boot && !c.gpu.id.is_empty())
        .map(|c| (c.gpu.clone(), "boot VGA device"))
}

pub(super) fn pick(backend: Backend, cards: &[Card]) -> Option<(Gpu, &'static str)> {
    pick_in(
        backend,
        cards,
        || gpu_from_dev(Path::new("/sys"), wayland_main_device()?),
        aq_gpu,
        xwayland_gpu,
    )
}

/// Ask the X server's selected screen, not the enclosing Wayland compositor (they can differ).
/// GPUI already uses this same DRI3 hint, but only *after* we narrowed the Vulkan ICDs.
fn xwayland_gpu() -> Option<Gpu> {
    use x11rb::connection::{Connection, RequestConnection};
    use x11rb::protocol::dri3;
    let (conn, screen) = x11rb::connect(None).ok()?;
    conn.extension_information(dri3::X11_EXTENSION_NAME)
        .ok()??;
    let root = conn.setup().roots.get(screen)?.root;
    let reply = dri3::open(&conn, root, 0).ok()?.reply().ok()?;
    let meta = std::fs::File::from(reply.device_fd).metadata().ok()?;
    meta.file_type().is_char_device().then_some(())?;
    gpu_from_dev(Path::new("/sys"), meta.rdev())
}

use wayland_client::protocol::{wl_callback, wl_registry};
use wayland_client::{Connection, Dispatch, QueueHandle, delegate_noop};
use wayland_protocols::wp::linux_dmabuf::zv1::client::{
    zwp_linux_dmabuf_feedback_v1, zwp_linux_dmabuf_v1,
};

#[derive(Default)]
struct Feedback {
    device: Option<libc::dev_t>,
    bound: bool,
    registry_done: bool,
    done: bool,
}

impl Dispatch<wl_registry::WlRegistry, ()> for Feedback {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = event
            && interface == "zwp_linux_dmabuf_v1"
            && version >= 4
            && !state.bound
        {
            // Bind exactly v4: v6 no longer sends main_device.
            let dmabuf =
                registry.bind::<zwp_linux_dmabuf_v1::ZwpLinuxDmabufV1, _, _>(name, 4, qh, ());
            dmabuf.get_default_feedback(qh, ());
            state.bound = true;
        }
    }
}

impl Dispatch<wl_callback::WlCallback, ()> for Feedback {
    fn event(
        state: &mut Self,
        _: &wl_callback::WlCallback,
        _: wl_callback::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        state.registry_done = true;
    }
}

delegate_noop!(Feedback: ignore zwp_linux_dmabuf_v1::ZwpLinuxDmabufV1);

fn decode_dev(bytes: &[u8]) -> Option<libc::dev_t> {
    Some(libc::dev_t::from_ne_bytes(bytes.try_into().ok()?))
}

impl Dispatch<zwp_linux_dmabuf_feedback_v1::ZwpLinuxDmabufFeedbackV1, ()> for Feedback {
    fn event(
        state: &mut Self,
        _: &zwp_linux_dmabuf_feedback_v1::ZwpLinuxDmabufFeedbackV1,
        event: zwp_linux_dmabuf_feedback_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            zwp_linux_dmabuf_feedback_v1::Event::MainDevice { device } => {
                state.device = decode_dev(&device)
            }
            zwp_linux_dmabuf_feedback_v1::Event::Done => state.done = true,
            // In particular, drop the format-table OwnedFd without mapping it: only the device matters.
            _ => {}
        }
    }
}

fn wayland_socket(display: &std::ffi::OsStr, runtime: Option<PathBuf>) -> Option<PathBuf> {
    let display = Path::new(display);
    if display.as_os_str().is_empty() {
        None
    } else if display.is_absolute() {
        Some(display.to_path_buf())
    } else {
        Some(runtime.filter(|p| p.is_absolute())?.join(display))
    }
}

/// A separate connection, no surfaces, no threads, and at most 300 ms waiting for feedback.
/// Don't consume WAYLAND_SOCKET: that FD (if supplied) belongs to GPUI's eventual connection.
fn wayland_main_device() -> Option<libc::dev_t> {
    use std::os::fd::AsRawFd;
    use std::os::unix::net::UnixStream;
    use std::time::{Duration, Instant};
    let path = wayland_socket(
        &std::env::var_os("WAYLAND_DISPLAY")?,
        std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from),
    )?;
    let conn = Connection::from_socket(UnixStream::connect(path).ok()?).ok()?;
    let mut queue = conn.new_event_queue();
    let qh = queue.handle();
    conn.display().get_registry(&qh, ());
    conn.display().sync(&qh, ());
    let mut state = Feedback::default();
    let deadline = Instant::now() + Duration::from_millis(300);
    loop {
        queue.dispatch_pending(&mut state).ok()?;
        if state.done {
            return state.device;
        }
        if state.registry_done && !state.bound {
            return None;
        }
        let remaining = deadline.checked_duration_since(Instant::now())?;
        conn.flush().ok()?;
        let Some(guard) = queue.prepare_read() else {
            continue;
        };
        let mut fd = libc::pollfd {
            fd: guard.connection_fd().as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: fd points to one valid pollfd; guard keeps the connection FD alive.
        let ready = unsafe { libc::poll(&mut fd, 1, remaining.as_millis().max(1) as i32) };
        if ready < 0 && std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
            continue;
        }
        if ready <= 0 || fd.revents & libc::POLLIN == 0 {
            return None;
        }
        guard.read().ok()?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn intel() -> Gpu {
        Gpu {
            id: "0x7d67".into(),
            vendor: "0x8086".into(),
        }
    }
    fn nvidia() -> Gpu {
        Gpu {
            id: "0x2f18".into(),
            vendor: "0x10de".into(),
        }
    }
    fn hybrid() -> Vec<Card> {
        vec![
            Card {
                gpu: nvidia(),
                boot: false,
            },
            Card {
                gpu: intel(),
                boot: true,
            },
        ]
    }

    #[test]
    fn dev_t_resolves_primary_and_render_nodes_in_sysfs() {
        let root = std::env::temp_dir().join(format!("capri-gpu-sysfs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let pci = root.join("devices/pci0000:00/0000:01:00.0");
        std::fs::create_dir_all(&pci).unwrap();
        std::fs::write(pci.join("device"), "0x2F18\n").unwrap();
        std::fs::write(pci.join("vendor"), "0x10de\n").unwrap();
        // High minor bits matter too; don't hand-roll an 8-bit major/minor split.
        for minor in [0, 128, 513] {
            let node = root.join(format!("dev/char/226:{minor}"));
            std::fs::create_dir_all(&node).unwrap();
            std::os::unix::fs::symlink(&pci, node.join("device")).unwrap();
            let dev = libc::makedev(226, minor);
            assert_eq!(gpu_from_dev(&root, dev), Some(nvidia()));
            assert_eq!(decode_dev(&dev.to_ne_bytes()), Some(dev));
        }
        assert_eq!(gpu_from_dev(&root, libc::makedev(226, 9)), None);
        std::fs::write(pci.join("device"), "not hex").unwrap();
        assert_eq!(gpu_from_dev(&root, libc::makedev(226, 0)), None);
        std::fs::write(pci.join("device"), "0x10000").unwrap();
        assert_eq!(gpu_from_dev(&root, libc::makedev(226, 0)), None);
        std::fs::write(pci.join("device"), "0x2f18").unwrap();
        std::fs::remove_file(pci.join("vendor")).unwrap();
        assert_eq!(gpu_from_dev(&root, libc::makedev(226, 0)), None);
        assert_eq!(decode_dev(&[]), None);
        assert_eq!(decode_dev(&[0; 3]), None);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn boot_vga_inventory_keeps_the_x11_rule() {
        let root = std::env::temp_dir().join(format!("capri-gpu-cards-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for (name, id, boot, vendor) in [
            ("card0", "0x2f18", "0", "0x10de"),
            ("card1", "0x7d67", "1", "0x8086"),
            ("card1-eDP-1", "0x7d67", "1", "0x8086"),
            ("renderD128", "0x2f18", "0", "0x10de"),
        ] {
            let device = root.join(name).join("device");
            std::fs::create_dir_all(&device).unwrap();
            for (file, text) in [("device", id), ("boot_vga", boot), ("vendor", vendor)] {
                std::fs::write(device.join(file), format!("{text}\n")).unwrap();
            }
        }
        let never = || panic!("X11 must not probe either display server");
        let cards = cards_in(&root);
        assert_eq!(cards.len(), 2);
        assert_eq!(
            pick_in(Backend::X11, &cards, never, never, never),
            Some((intel(), "boot VGA device"))
        );
        // A DRM card without PCI IDs still counts, just as in the previous X11 code.
        std::fs::remove_dir_all(root.join("card0/device")).unwrap();
        let cards = cards_in(&root);
        assert_eq!(cards.len(), 2);
        assert_eq!(
            pick_in(Backend::X11, &cards, never, never, never),
            Some((intel(), "boot VGA device"))
        );
        std::fs::remove_file(root.join("card1/device/device")).unwrap();
        assert_eq!(
            pick_in(Backend::X11, &cards_in(&root), never, never, never),
            None
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn aquamarine_uses_only_the_first_entry() {
        for value in [
            "/dev/dri/gpu-nvidia:/dev/dri/gpu-intel",
            " /dev/dri/gpu-nvidia : /dev/dri/gpu-intel",
            "/dev/dri/gpu-nvidia:",
        ] {
            assert_eq!(aq_first(value), Some(Path::new("/dev/dri/gpu-nvidia")));
        }
        assert_eq!(
            aq_first("/dev/dri/card0"),
            Some(Path::new("/dev/dri/card0"))
        );
        for value in ["", " ", ":/dev/dri/card1", "card0:/dev/dri/card1"] {
            assert_eq!(aq_first(value), None, "{value:?}");
        }
    }

    #[test]
    fn selection_precedence_and_backend_isolation() {
        let cards = hybrid();
        let never = || panic!("probe must not run");
        assert_eq!(
            pick_in(Backend::Wayland, &cards, || Some(nvidia()), never, never),
            Some((nvidia(), "Wayland linux-dmabuf main_device"))
        );
        assert_eq!(
            pick_in(Backend::Wayland, &cards, || None, || Some(nvidia()), never),
            Some((nvidia(), "AQ_DRM_DEVICES first entry"))
        );
        assert_eq!(
            pick_in(Backend::Wayland, &cards, || None, || None, never),
            Some((intel(), "boot VGA device"))
        );
        assert_eq!(
            pick_in(Backend::X11, &cards, never, never, never),
            Some((intel(), "boot VGA device"))
        );
        for backend in [Backend::XWayland, Backend::XWaylandAuto] {
            assert_eq!(
                pick_in(backend, &cards, never, never, || Some(nvidia())),
                Some((nvidia(), "XWayland screen (DRI3)"))
            );
            assert_eq!(
                pick_in(backend, &cards, never, never, || None),
                Some((intel(), "boot VGA device"))
            );
        }
        assert_eq!(
            pick_in(Backend::X11, &cards[1..], never, never, never),
            None
        );
        assert_eq!(
            pick_in(Backend::Wayland, &[], || None, || None, never),
            None
        );
        assert_eq!(
            pick_in(Backend::Wayland, &[], || Some(nvidia()), never, never),
            Some((nvidia(), "Wayland linux-dmabuf main_device"))
        );
        assert_eq!(
            pick_in(
                Backend::X11,
                &[
                    Card {
                        gpu: intel(),
                        boot: false
                    },
                    Card {
                        gpu: nvidia(),
                        boot: false
                    }
                ],
                never,
                never,
                never
            ),
            None
        );
    }

    #[test]
    fn independent_wayland_socket_path() {
        use std::ffi::OsStr;
        assert_eq!(
            wayland_socket(OsStr::new("wayland-1"), Some("/run/user/1000".into())),
            Some("/run/user/1000/wayland-1".into())
        );
        assert_eq!(
            wayland_socket(OsStr::new("/tmp/wayland-2"), None),
            Some("/tmp/wayland-2".into())
        );
        assert_eq!(wayland_socket(OsStr::new("wayland-1"), None), None);
        assert_eq!(
            wayland_socket(OsStr::new("wayland-1"), Some("relative".into())),
            None
        );
        assert_eq!(
            wayland_socket(OsStr::new(""), Some("/run/user/1000".into())),
            None
        );
    }
}

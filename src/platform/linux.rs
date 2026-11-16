//! The Linux process: what has to be set before GPUI starts (GPU choice, the allocator).
//! Shared by every Linux backend (X11, Hyprland).

/// Process setup that has to happen before any other thread exists. Each knob was
/// measured on bella (release, `ps` RSS 6 s after start, see LOG.md):
///
/// * Render on the boot VGA device (the iGPU on a hybrid laptop) unless `ZED_DEVICE_ID` is
///   set. On bella every vkQueuePresent to this 32-bit override-redirect window
///   busy-waited ~2.3 s on the NVIDIA dGPU (0.4 fps); the Intel iGPU presents at once. A
///   desktop pet belongs on the low-power GPU anyway.
/// * Keep GPUI's GL fallback from probing EGL when a Vulkan driver is installed. The probe
///   loads every EGL vendor (NVIDIA's, Mesa's llvmpipe with LLVM) and 50 threads that stay
///   for the life of the process: 371 → 226 MB RSS. `SA_GL=1` keeps it.
/// * glibc malloc: two arenas and a 1 MiB trim threshold, so the full-size PNG decode
///   buffers go back to the OS after scaling: 226 → 84 MB.
pub fn process_setup() {
    // SA_BACKEND=x11 on a Wayland desktop: GPUI and we both take X11 (XWayland).
    if std::env::var("SA_BACKEND").is_ok_and(|v| v == "x11") && std::env::var_os("WAYLAND_DISPLAY").is_some() {
        if std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some() {
            elog!("synaps-avatar: SA_BACKEND=x11 on Hyprland: through XWayland she sees X clients only, and can't stay above native windows");
        }
        // SAFETY: first thing in main, single-threaded.
        unsafe { std::env::remove_var("WAYLAND_DISPLAY") };
    }
    if std::env::var_os("SA_GL").is_none() && has_vulkan_driver() {
        // SAFETY (all set_var calls here): first thing in main, single-threaded.
        unsafe { std::env::set_var("__EGL_VENDOR_LIBRARY_DIRS", "/nonexistent") };
    }
    #[cfg(target_env = "gnu")]
    // SAFETY: plain libc calls with valid parameters.
    unsafe {
        libc::mallopt(libc::M_ARENA_MAX, 2);
        libc::mallopt(libc::M_TRIM_THRESHOLD, 1 << 20);
    }
    if std::env::var_os("ZED_DEVICE_ID").is_some() || std::env::var_os("SA_ANY_GPU").is_some() {
        return;
    }
    let read = |p: std::path::PathBuf| std::fs::read_to_string(p).map(|s| s.trim().to_string()).unwrap_or_default();
    let mut cards = Vec::new();
    for e in std::fs::read_dir("/sys/class/drm").into_iter().flatten().flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if !name.starts_with("card") || name.contains('-') {
            continue;
        }
        let dev = e.path().join("device");
        cards.push((read(dev.join("device")), read(dev.join("boot_vga")) == "1", read(dev.join("vendor"))));
    }
    if cards.len() < 2 {
        return;
    }
    if let Some((id, _, vendor)) = cards.iter().find(|c| c.1 && !c.0.is_empty()) {
        elog!("synaps-avatar: {} GPUs, rendering on the boot VGA device {id} (SA_ANY_GPU=1 to let GPUI pick)", cards.len());
        unsafe { std::env::set_var("ZED_DEVICE_ID", id) };
        // Only that vendor's Vulkan driver: the loader otherwise maps every ICD, and the
        // unused NVIDIA one alone was ~52 MB of RSS (libnvidia-gpucomp/rtcore/glcore).
        if std::env::var_os("VK_DRIVER_FILES").is_none()
            && std::env::var_os("VK_ICD_FILENAMES").is_none()
            && let Some(files) = vendor_icds(vendor)
        {
            unsafe { std::env::set_var("VK_DRIVER_FILES", &files) };
        }
    }
}

/// The Vulkan driver manifests of a PCI vendor (`0x8086` …), joined for `VK_DRIVER_FILES`.
fn vendor_icds(vendor: &str) -> Option<String> {
    let prefixes: &[&str] = match vendor {
        "0x8086" => &["intel"],
        "0x1002" => &["radeon", "amd"],
        "0x10de" => &["nvidia"],
        _ => return None,
    };
    let data_dirs = std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".into());
    let mut dirs: Vec<std::path::PathBuf> = data_dirs.split(':').map(|d| std::path::Path::new(d).join("vulkan/icd.d")).collect();
    dirs.push("/etc/vulkan/icd.d".into());
    let mut files: Vec<String> = Vec::new();
    for d in dirs {
        for e in std::fs::read_dir(d).into_iter().flatten().flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.ends_with(".json") && prefixes.iter().any(|p| name.starts_with(p)) {
                files.push(e.path().to_string_lossy().into_owned());
            }
        }
    }
    files.sort();
    files.dedup();
    (!files.is_empty()).then(|| files.join(":"))
}

/// Hand freed heap back to the OS (after the frame decode).
pub fn trim_heap() {
    #[cfg(target_env = "gnu")]
    // SAFETY: plain libc call.
    unsafe {
        libc::malloc_trim(0);
    }
}

/// Whether the Vulkan loader will find at least one driver manifest.
fn has_vulkan_driver() -> bool {
    let mut dirs: Vec<std::path::PathBuf> = Vec::new();
    for var in ["VK_DRIVER_FILES", "VK_ICD_FILENAMES"] {
        if let Some(v) = std::env::var_os(var) {
            return std::env::split_paths(&v).any(|p| p.exists());
        }
    }
    let data_dirs = std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".into());
    dirs.extend(data_dirs.split(':').map(|d| std::path::Path::new(d).join("vulkan/icd.d")));
    dirs.push("/etc/vulkan/icd.d".into());
    dirs.iter().any(|d| std::fs::read_dir(d).into_iter().flatten().flatten().any(|e| e.path().extension().is_some_and(|x| x == "json")))
}

/// CLOCK_MONOTONIC seconds (the same clock as Python's `time.perf_counter`: dev logs line up
/// with the test tools').
pub fn monotonic_s() -> f64 {
    let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    // SAFETY: a plain clock read into a local.
    unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts) };
    ts.tv_sec as f64 + ts.tv_nsec as f64 * 1e-9
}

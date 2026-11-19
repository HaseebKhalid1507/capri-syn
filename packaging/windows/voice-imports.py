"""The voice bundle's native imports: each one found on a clean Windows 10/11 (voice-bundle.ps1 runs it
on the bundle it built; test-voice-install.ps1 on what Setup installed, with --live).

    python voice-imports.py BUNDLE [--system32 DIR] [--live]

Reads the import table and the delay-load table of every .dll, .pyd and .exe under BUNDLE (PE files,
read here: no dumpbin or pefile, so it runs off Windows too) and looks for each DLL they name where
Windows' loader does when Python loads them, in its order. Extension modules (.pyd) and the DLLs ctypes
loads by path (espeak-ng, PortAudio) are loaded with LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | _DEFAULT_DIRS,
which search:
  - the folder of the file that imports it;
  - BUNDLE\\python\\: python.exe's folder, the application's;
  - the folders packages add with os.add_dll_directory: site-packages\\<name>.libs\\ (delvewheel's,
    numpy's) for the files under site-packages\\<name>\\;
  - System32.
A delay-load is loaded with LoadLibraryExA(name, NULL, 0): python\\, then System32.

System32 counts only for a name in CLEAN_SYSTEM32, what a clean Windows 10 1809+ has (on Windows, also
found in --system32 DIR, default %SystemRoot%\\System32): the build machine's System32 also holds what
Visual Studio and the Windows SDKs put there. Never for the Visual C++ runtime (msvcp140*, vcruntime140*,
vcomp140, concrt140, ...): GitHub's runners and any PC with Visual Studio have the VC++ Redistributable
in System32, a clean Windows doesn't. The bundle carries each of those it imports in python\\
(voice-bundle.ps1), and it is one copy per process by name: python\\'s, never one beside a module.
API sets (api-ms-win-*, ext-ms-win-*) are Windows' own.

And the VC++ runtime in python\\ must be at least as new as the MSVC that linked each file importing it
(Microsoft: the runtime must be the same or later than the newest build tools in the app; onnxruntime
built with 14.40+ crashes on an older msvcp140, std::mutex). The measure is rough: the PE header's
linker version. MSVC's link.exe writes its own; lld-link writes 14.0 unless told otherwise, so a
clang-built DLL's floor means nothing (GNU ld writes 2.x: the MinGW builds, OpenBLAS and protobuf's,
don't import the VC++ runtime). And renamed copies (numpy's and pywhispercpp's msvcp140-<hash>.dll)
aren't floored: pywhispercpp's whisper and ggml DLLs, linked by 14.44, run on its 14.40.33810 copy.
That one is at the 14.40 std::mutex change, and --live loads them.

--live (Windows; run by the bundle's own python.exe): then imports what the sidecar does and loads
espeak-ng's DLL as phonemizer does, and checks the VC++ runtime this process loaded is the bundle's
(python\\), not System32's: msvcp140.dll and msvcp140_1.dll among it (onnxruntime's).

Exits 1, naming each file and the DLL it can't find.
"""

import argparse
import os
import re
import struct
import sys
from pathlib import Path

# The Visual C++ Redistributable's DLLs: never counted as System32's.
# (ucrtbased: the debug UCRT, which the Windows SDK puts in the runner's System32.)
VC_RUNTIME = re.compile(r"^(msvcp|vcruntime|vcomp|concrt|vccorlib|vcamp|mfc|mfcm)\d+[a-z0-9_]*\.dll$|^msvcr\d+\.dll$|^ucrtbased\.dll$", re.I)
API_SET = re.compile(r"^(api|ext)-ms-", re.I)
# What a clean Windows 10 1809+ (the installers' MinVersion) has in System32, by name: the bundle's
# imports as of #113 (unins000.exe's too). The build machine's System32 also holds what Visual Studio and
# the SDKs put there, so a new name stops the build until someone checks a clean Windows has it.
CLEAN_SYSTEM32 = {
    "advapi32.dll", "bcrypt.dll", "comctl32.dll", "comdlg32.dll", "crypt32.dll", "dbghelp.dll", "dwmapi.dll",
    "dxgi.dll", "gdi32.dll", "imm32.dll", "iphlpapi.dll", "kernel32.dll", "mpr.dll", "msimg32.dll", "msvcrt.dll",
    "ntdll.dll", "ole32.dll", "oleaut32.dll", "pdh.dll", "powrprof.dll", "propsys.dll", "psapi.dll", "rpcrt4.dll",
    "setupapi.dll", "shcore.dll", "shell32.dll", "shlwapi.dll", "user32.dll", "uxtheme.dll", "version.dll",
    "windowscodecs.dll", "winhttp.dll", "winmm.dll", "winspool.drv", "ws2_32.dll", "wtsapi32.dll",
}


def file_version(path):
    """A PE file's version (VS_FIXEDFILEINFO: what Explorer and PowerShell's FileVersionRaw show), or None."""
    b = path.read_bytes()
    i = b.find(b"\xbd\x04\xef\xfe")       # its signature, 0xFEEF04BD
    if i < 0:
        return None
    ms, ls = struct.unpack_from("<II", b, i + 8)
    return (ms >> 16, ms & 0xFFFF, ls >> 16, ls & 0xFFFF)


def imports(path):
    """(the DLLs `path` imports, those it delay-loads, its linker's (major, minor)), from its PE
    headers; ([], [], None) for a file that isn't a PE image. Raises ValueError on a malformed one."""
    b = path.read_bytes()
    if len(b) < 0x40 or b[:2] != b"MZ":
        return [], [], None
    pe = struct.unpack_from("<I", b, 0x3C)[0]
    if b[pe:pe + 4] != b"PE\0\0":
        raise ValueError("no PE signature")
    nsec, opt_size = struct.unpack_from("<H12xH", b, pe + 6)
    opt = pe + 24
    magic, linker = struct.unpack_from("<H", b, opt)[0], (b[opt + 2], b[opt + 3])
    if magic == 0x20B:      # PE32+
        image_base, ndirs, dirs = struct.unpack_from("<Q", b, opt + 24)[0], struct.unpack_from("<I", b, opt + 108)[0], opt + 112
    elif magic == 0x10B:    # PE32
        image_base, ndirs, dirs = struct.unpack_from("<I", b, opt + 28)[0], struct.unpack_from("<I", b, opt + 92)[0], opt + 96
    else:
        raise ValueError(f"optional header magic {magic:#x}")
    sections = [struct.unpack_from("<8xIIII", b, opt + opt_size + 40 * i) for i in range(nsec)]

    def off(rva):
        for vsize, va, raw_size, raw in sections:
            if va <= rva < va + max(vsize, raw_size):
                return rva - va + raw
        raise ValueError(f"RVA {rva:#x} in no section")

    def name(rva):
        o = off(rva)
        return b[o:b.index(b"\0", o)].decode("ascii")

    def directory(i):
        return struct.unpack_from("<II", b, dirs + 8 * i) if i < ndirs else (0, 0)

    normal, delayed = [], []
    rva, size = directory(1)            # IMAGE_DIRECTORY_ENTRY_IMPORT: 20-byte descriptors, a zero one last
    if rva and size:
        o = off(rva)
        while True:
            name_rva, first_thunk = struct.unpack_from("<12xII", b, o)
            if not name_rva and not first_thunk:
                break
            normal.append(name(name_rva))
            o += 20
    rva, size = directory(13)           # IMAGE_DIRECTORY_ENTRY_DELAY_IMPORT: 32-byte descriptors
    if rva and size:
        o = off(rva)
        while True:
            attrs, name_ref = struct.unpack_from("<II", b, o)
            if not name_ref:
                break
            delayed.append(name(name_ref if attrs & 1 else name_ref - image_base))   # bit 0: RVAs, else VAs
            o += 32
    return normal, delayed, linker


def search(bundle, f, delay=False):
    """The bundle's folders the loader looks in for what `f` imports, in its order. A module Python loads
    (LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | DEFAULT_DIRS): the importer's folder, python\\ (the application's),
    then os.add_dll_directory's (site-packages\\<name>.libs\\). A delay-load (LoadLibraryExA(name, NULL, 0)):
    python\\ only, before System32."""
    if delay:
        return [bundle / "python"]
    dirs = [f.parent, bundle / "python"]
    site = bundle / "site-packages"
    rel = f.relative_to(site).parts if f.is_relative_to(site) else ()
    if len(rel) > 1:
        top = rel[0][:-5] if rel[0].endswith(".libs") else rel[0]
        dirs.append(site / f"{top}.libs")
    return dirs


def loaded():
    """The files of the modules this process has loaded (Windows)."""
    import ctypes
    from ctypes import wintypes
    k = ctypes.WinDLL("kernel32", use_last_error=True)
    k.GetCurrentProcess.restype = wintypes.HANDLE
    k.K32EnumProcessModules.argtypes = [wintypes.HANDLE, ctypes.POINTER(ctypes.c_void_p), wintypes.DWORD, ctypes.POINTER(wintypes.DWORD)]
    k.GetModuleFileNameW.argtypes = [ctypes.c_void_p, wintypes.LPWSTR, wintypes.DWORD]
    mods, need = (ctypes.c_void_p * 4096)(), wintypes.DWORD()
    if not k.K32EnumProcessModules(k.GetCurrentProcess(), mods, ctypes.sizeof(mods), ctypes.byref(need)):
        raise OSError(ctypes.get_last_error(), "EnumProcessModules")
    buf = ctypes.create_unicode_buffer(32768)
    return [Path(buf.value) for h in mods[:need.value // ctypes.sizeof(ctypes.c_void_p)] if k.GetModuleFileNameW(h, buf, 32768)]


def live(bundle):
    """--live: what the sidecar loads, then whose VC++ runtime is in this process."""
    import importlib
    for m in ("numpy", "onnxruntime", "sounddevice", "pywhispercpp.model", "kokoro_onnx", "psutil",
              "pycaw.pycaw", "winrt.windows.media.control"):
        importlib.import_module(m)
    import espeakng_loader
    from phonemizer.backend.espeak.wrapper import EspeakWrapper
    EspeakWrapper.set_library(espeakng_loader.get_library_path())
    EspeakWrapper.set_data_path(espeakng_loader.get_data_path())
    # phonemizer's own load, as kokoro's tokenizer makes it: a copy in %TEMP%, by path. From there
    # msvcp140 comes from python\ (the application's folder) or what is loaded already.
    print(f"  espeak-ng {'.'.join(map(str, EspeakWrapper().version))}, loaded as phonemizer does")
    inside = os.path.normcase(os.path.realpath(bundle)) + os.sep      # long names on both sides (no RUNNER~1)
    vc = sorted((p for p in loaded() if VC_RUNTIME.match(p.name)), key=lambda p: p.name.lower())
    for p in vc:
        print(f"  loaded: {p}")
    bad = [str(p) for p in vc if not os.path.normcase(os.path.realpath(p)).startswith(inside)]
    names = {p.name.lower() for p in vc}
    bad += [f"{n}: not loaded (onnxruntime imports it)" for n in ("msvcp140.dll", "msvcp140_1.dll") if n not in names]
    if bad:
        print(f"voice-imports: the VC++ runtime this process runs on isn't the bundle's ({bundle}): " + "; ".join(bad), file=sys.stderr)
        sys.exit(1)
    print(f"voice imports, live: the VC++ runtime loaded is the bundle's ({len(vc)} DLLs)")


def main(argv):
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("bundle", type=Path)
    ap.add_argument("--system32", type=Path)
    ap.add_argument("--live", action="store_true")
    a = ap.parse_args(argv)
    bundle = a.bundle.resolve()
    system32 = a.system32 or (Path(os.environ["SystemRoot"]) / "System32" if os.name == "nt" else None)
    if not (bundle / "python" / "python.exe").is_file():
        sys.exit(f"voice-imports: no python\\python.exe in {bundle}")
    lower = {}                          # each folder's files, by lower-case name (Windows matches names case-blind)
    def has(d, n):
        if d not in lower:
            lower[d] = {p.name.lower() for p in d.iterdir()} if d.is_dir() else set()
        return n.lower() in lower[d]

    versions = {}
    def version(d, n):
        p = next(q for q in d.iterdir() if q.name.lower() == n.lower())
        if p not in versions:
            versions[p] = file_version(p)
        return versions[p]

    files = sorted(p for p in bundle.rglob("*") if p.is_file() and p.suffix.lower() in (".dll", ".pyd", ".exe"))
    missing, system, vc, n = [], set(), {}, 0
    for f in files:
        try:
            normal, delayed, linker = imports(f)
        except (ValueError, struct.error, IndexError, UnicodeDecodeError) as e:
            missing.append(f"{f.relative_to(bundle)}: can't read its imports ({e})")
            continue
        # Every extension module links Python's DLL: one that seems not to was misread (no silent pass).
        if f.suffix.lower() == ".pyd" and not any(re.match(r"python3\d*\.dll$", d, re.I) for d in normal):
            missing.append(f"{f.relative_to(bundle)}: no python3*.dll among its imports ({', '.join(normal) or 'none read'}): misread?")
        for dll, delay in [(d, False) for d in normal] + [(d, True) for d in delayed]:
            n += 1
            if API_SET.match(dll):
                continue
            how = " (delay-loaded)" if delay else ""
            found = next((d for d in search(bundle, f, delay) if has(d, dll)), None)
            if VC_RUNTIME.match(dll):
                # By its own name, one copy per process: python\'s, never one beside a module, never System32's.
                if found != bundle / "python":
                    where = f"it would load {found.relative_to(bundle)}'s" if found else "python\\ has none"
                    missing.append(f"{f.relative_to(bundle)} -> {dll}{how}: the Visual C++ runtime ({where}); it must be python\\'s")
                    continue
                vc.setdefault(dll.lower(), set()).add(f.name)
                have = version(found, dll)
                if have and have[:2] < linker:
                    missing.append(f"{f.relative_to(bundle)} -> {dll}: linked by MSVC {linker[0]}.{linker[1]}, but python\\{dll} is "
                                   f"{'.'.join(map(str, have))}: the runtime must be at least as new")
                continue
            if found is not None:
                continue
            if dll.lower() not in CLEAN_SYSTEM32:
                missing.append(f"{f.relative_to(bundle)} -> {dll}{how}: not in the bundle, nor in CLEAN_SYSTEM32 "
                               f"(check a clean Windows 10 1809 has it, then add it)")
            elif system32 is not None and not has(system32, dll):
                missing.append(f"{f.relative_to(bundle)} -> {dll}{how}: not in the bundle, nor in {system32}")
            else:
                system.add(dll.lower())

    print(f"voice imports: {len(files)} DLLs/modules/exes, {n} imports")
    for dll in sorted(vc):
        v = version(bundle / "python", dll) if has(bundle / "python", dll) else None
        print(f"  {dll}{' ' + '.'.join(map(str, v)) if v else ''} (VC++ runtime, from the bundle): "
              f"{', '.join(sorted(vc[dll])[:6])}{' ...' if len(vc[dll]) > 6 else ''}")
    print(f"  System32's (CLEAN_SYSTEM32{'' if system32 else '; not on Windows, so not looked for'}): {', '.join(sorted(system))}")
    if missing:
        print("voice-imports: what the bundle imports that a clean Windows wouldn't find, or too old a runtime:", file=sys.stderr)
        for m in missing:
            print("  " + m, file=sys.stderr)
        sys.exit(1)
    if a.live:
        if os.name != "nt":
            sys.exit("voice-imports: --live runs on Windows only")
        live(bundle)


if __name__ == "__main__":
    main(sys.argv[1:])

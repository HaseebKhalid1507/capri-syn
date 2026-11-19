"""The voice bundle's licences (voice-bundle.ps1 runs it once the wheels are in).

    python voice-licenses.py BUNDLE

writes BUNDLE/licenses/:
  THIRD-PARTY-LICENSES.txt  every package in BUNDLE/site-packages: its version, its declared licence,
                            where its source is, and the licence texts its wheel carries
  SOURCE.txt                where the source of the GPL-licensed parts is (GPL-3.0 section 6): the
                            release this installer came from carries it (release.yml's assemble)
  espeak-ng/, whisper.cpp/, portaudio/
                            the licence files of what three wheels carry inside them without one
                            (packaging/windows/voice-licenses/)
  vc-runtime/LICENSE.txt    Microsoft's end-user terms for the Visual C++ runtime voice-bundle.ps1
                            puts in python/: what the user's use of it is under. Our right to ship it
                            is the Visual Studio licence's (VC_RUNTIME's comment).

espeak-ng's DLL comes inside espeakng-loader's wheel, whisper.cpp's and ggml's DLLs inside
pywhispercpp's, PortAudio's inside sounddevice's; none of the three has their licence text. All three
are pinned (tools/capri-voice-win-requirements.txt) and checked here against the versions INSIDE was
written for, and the espeak-ng and PortAudio DLLs against the versions named: a new wheel can't ship
under a stale notice.

It also stops the bundle on what nobody has looked at: a native binary (DLL or exe) not in NATIVE (a
debug C runtime among them), a package whose declared licence isn't on ACCEPTED or names a REFUSED
one beside it, a new GPL-family package, or a compiled one (SOURCE.txt calls the GPL packages their
own source); a VC++ runtime in python/ that isn't VC_RUNTIME, or isn't the v14 runtime its terms are
for.
"""

import re
import shutil
import struct
import sys
import textwrap
from importlib import metadata
from pathlib import Path

# The espeak-ng in espeakng-loader 0.2.4's Windows wheel (tools/capri-voice-win-requirements.txt pins
# it): the version its DLL reports, and that tag's commit in espeak-ng's repository.
ESPEAK_NG = "1.52.0"
ESPEAK_NG_COMMIT = "4870adfa25b1a32b4361592f1be8a40337c58d6c"
LOADER = "0.2.4"
# espeakng-loader 0.2.4's commit (untagged): its build.sh and workflows built the DLL.
LOADER_COMMIT = "146599e29be31bf17d99f0bcb7dbb2f92aef3d95"
# GPL-3.0 section 6(d): the Corresponding Source beside the object code, as assets of the release this
# installer came from (release.yml's assemble attaches them, SHA-256 pinned there).
ESPEAK_NG_SOURCE = f"espeak-ng-{ESPEAK_NG}.tar.gz"
LOADER_SOURCE = f"espeakng-loader-{LOADER}-build.tar.gz"
# espeak-ng's CMake (cmake/deps.cmake) clones Bill Cox's sonic at this commit and compiles sonic.c into the
# DLL (its sinc table is in espeak-ng.dll): Apache-2.0, and part of the DLL's Corresponding Source.
SONIC_COMMIT = "fbf75c3d6d846bad3bb3d456cbc5d07d9fd8c104"
SONIC_SOURCE = f"sonic-{SONIC_COMMIT[:7]}.tar.gz"
# The PortAudio in sounddevice 0.5.6's wheel: what its DLL reports (spatialaudio/portaudio-binaries
# builds the v19.7.0 tag).
PORTAUDIO = "19.7.0"
# The Visual C++ runtime voice-bundle.ps1 puts beside python.exe (onnxruntime's and espeak-ng's DLLs
# import it by name; a clean Windows doesn't have it): from Visual Studio's VC\Redist\MSVC\<version>\
# x64\Microsoft.VC14x.CRT, unmodified, the embeddable's vcruntime140*.dll kept when newer. Our right to
# ship it: the Visual Studio licence's Distributable Code (its Distributable List,
# https://aka.ms/vs/18/redistribution, names VC\Redist), which requires end users to take terms that
# protect it at least as much. Those terms, voice-licenses/vc-runtime/LICENSE.txt: Microsoft's end-user
# licence for the runtime ("Visual C++ v14 Redistributable and Runtime", https://aka.ms/VCRedistLicense,
# EULA ID Cpp_v14_ENU.1033, last updated October 1, 2025), as text. It grants use only and forbids
# passing the runtime on (4(e), (f)): what the user's use is under, not our permission. Written for the
# v14 runtime (14.x, Visual Studio 2017 to 2026).
VC_RUNTIME = ["msvcp140.dll", "msvcp140_1.dll", "vcruntime140.dll", "vcruntime140_1.dll"]
VC_RUNTIME_MAJOR = 14
# The VC++ runtime's DLL names, and its debug builds (not redistributable).
VC_NAME = re.compile(r"^(msvcp|vcruntime|vcomp|concrt|vccorlib|vcamp|mfc)\d+", re.I)
DEBUG_CRT = re.compile(r"^(msvcp|vcruntime|concrt|vcomp)\d+(_\w+)?d\.dll$|^ucrtbased\.dll$", re.I)

# What a wheel carries inside it without its licence text: (the wheel's version these lines were
# checked for, the lines, the folder of licence files under voice-licenses/ that goes with them).
INSIDE = {
    "espeakng-loader": (LOADER, [
        f"espeak-ng {ESPEAK_NG} (espeak-ng.dll, espeak-ng-data): GPL-3.0-or-later; its licence files are in",
        "licenses\\espeak-ng\\, and where its source is, in licenses\\SOURCE.txt",
        "sonic (compiled into espeak-ng.dll by espeak-ng's CMake): Apache-2.0, licenses\\espeak-ng\\sonic-LICENSE",
        "the loader itself (__init__.py): MIT, its repository's LICENSE (none in the wheel):",
        "https://github.com/thewh1teagle/espeakng-loader",
    ], "espeak-ng"),
    # pywhispercpp v1.5.1's whisper.cpp submodule: ggml-org/whisper.cpp at f24588a272ae.
    "pywhispercpp": ("1.5.1", [
        "whisper.cpp and ggml (whisper-*.dll, ggml-*.dll; whisper.cpp at f24588a272ae): MIT, licenses\\whisper.cpp\\LICENSE",
        "msvcp140-*.dll and vcomp140-*.dll: Microsoft's Visual C++ runtime, as its wheel ships them",
        "(Distributable Code under Microsoft's Visual Studio licence terms)",
    ], "whisper.cpp"),
    "sounddevice": ("0.5.6", [
        f"PortAudio v{PORTAUDIO} (_sounddevice_data\\portaudio-binaries\\libportaudio64bit.dll; the other",
        "platforms' builds and the ASIO ones are removed by voice-bundle.ps1): MIT-style, licenses\\portaudio\\LICENSE.txt",
    ], "portaudio"),
}

# Every native binary in site-packages, by whose it is (its licence: INSIDE, or its wheel's own). A new
# one stops the bundle until it's named here, with its licence. DLLs and exes; Python's own extension
# modules (.pyd) aren't listed yet. Release builds only: a debug C runtime
# (msvcp140d, vcruntime140d, ucrtbased: not redistributable) matches none of these.
NATIVE = [
    r"^numpy\.libs/(libscipy_openblas64_|msvcp140)-[0-9a-f]+\.dll$",  # numpy's: OpenBLAS (its LICENSE.txt), the VC++ runtime
    r"^(whisper|ggml|ggml-base|ggml-cpu|msvcp140|vcomp140)-[0-9a-f]+\.dll$",  # pywhispercpp's (INSIDE)
    r"^espeakng_loader/espeak-ng\.dll$",  # espeakng-loader's (INSIDE)
    r"^onnxruntime/capi/onnxruntime(_providers_shared)?\.dll$",  # onnxruntime's (its ThirdPartyNotices)
    r"^_sounddevice_data/portaudio-binaries/libportaudio64bit\.dll$",  # sounddevice's (INSIDE)
]
# The licences a package may declare (an SPDX expression, a License field, or its classifiers). Whole
# words: "MIT" is in "permitted", "ISC" in "disclaimer".
ACCEPTED = re.compile(r"\b(MIT|BSD|Apache|ISC|PSF|Python Software Foundation|MPL-2\.0|Mozilla Public|Unlicense|Zlib|CC0)\b", re.I)
# Refused even beside an accepted one ("MIT AND LicenseRef-Proprietary"). Declared names and licence
# classifiers only: Apache's full text says "commercial damages".
REFUSED = re.compile(r"Proprietary|Commercial|LicenseRef|Other/|SSPL|BUSL|Elastic|Commons Clause", re.I)
# Packages that declare none, read by hand from the licence file their wheel carries.
UNDECLARED = {"kokoro-onnx": "MIT"}
# GPL-family packages, each a decision: pure Python, so the bundle holds their source.
GPL_OK = {"phonemizer"}

HERE = Path(__file__).resolve().parent
LICENCE_NAME = re.compile(r"^(LICEN[CS]E|COPYING|NOTICE|COPYRIGHT|AUTHORS|THIRD[-_ ]?PARTY)", re.I)
APACHE = re.compile(r"^Apache(-| Software License| License,? )?(2\.0|Version 2\.0)?$|^Apache 2\.0$", re.I)
RULE = "=" * 100


def declared(dist):
    """The licence a package declares: its SPDX expression, else its License field (when short),
    else its trove classifiers."""
    m = dist.metadata
    if m.get("License-Expression"):
        return m["License-Expression"]
    lic = (m.get("License") or "").strip()
    classifiers = [c.split(" :: ")[-1] for c in m.get_all("Classifier") or [] if c.startswith("License ::")]
    if lic and "\n" not in lic and len(lic) <= 120:
        return lic
    if classifiers:
        return "; ".join(classifiers)
    return "(full text below)" if lic else "(none declared)"


def source_url(dist):
    m = dist.metadata
    urls = {}
    for u in m.get_all("Project-URL") or []:
        k, _, v = u.partition(",")
        urls[k.strip().lower()] = v.strip()
    for k in ("source", "source code", "repository", "code", "homepage", "home"):
        if urls.get(k):
            return urls[k]
    return m.get("Home-page") or ""


def licence_files(dist):
    """The licence texts in its wheel: all of its .dist-info/licenses/ (PEP 639: numpy lists its
    vendored parts' there, folders deep), and the licence-named files near the top of the rest."""
    found = []
    for f in dist.files or []:
        parts = Path(str(f)).parts
        pep639 = len(parts) > 2 and parts[0].endswith(".dist-info") and parts[1] == "licenses"
        if pep639 or (len(parts) <= 3 and LICENCE_NAME.match(parts[-1])):
            p = Path(dist.locate_file(f))
            if p.is_file():
                found.append((str(f).replace("\\", "/"), p))
    return sorted(set(found))


def file_version(path):
    """A PE file's version (its VS_FIXEDFILEINFO: what Explorer's Details and PowerShell's
    FileVersionRaw show), or None."""
    b = path.read_bytes()
    i = b.find(b"\xbd\x04\xef\xfe")       # its signature, 0xFEEF04BD
    if i < 0:
        return None
    ms, ls = struct.unpack_from("<II", b, i + 8)
    return (ms >> 16, ms & 0xFFFF, ls >> 16, ls & 0xFFFF)


def key(dist):
    return dist.metadata["Name"].lower().replace("_", "-")


def is_gpl(dist):
    text = " ".join([declared(dist), dist.metadata.get("License") or ""] + (dist.metadata.get_all("Classifier") or []))
    # LGPL-* and AGPL-* too: there's no word boundary inside "LGPL".
    return re.search(r"\b(?:A|L)?GPL|General Public License", text, re.I) is not None


def main(bundle):
    site = bundle / "site-packages"
    out = bundle / "licenses"
    out.mkdir(parents=True, exist_ok=True)
    dists = sorted(metadata.distributions(path=[str(site)]), key=lambda d: d.metadata["Name"].lower())
    if not dists:
        sys.exit(f"voice-licenses: no packages in {site}")
    names = {key(d): d for d in dists}

    # What wheels carry inside them: the versions INSIDE was written for, or stop.
    for name, (want, _, folder) in INSIDE.items():
        d = names.get(name)
        if d is None:
            sys.exit(f"voice-licenses: no {name} in the bundle (INSIDE names what it carries)")
        if d.version != want:
            sys.exit(f"voice-licenses: {name} {d.version}, but INSIDE is written for {want}: check what it carries, then update INSIDE")
        shutil.copytree(HERE / "voice-licenses" / folder, out / folder, dirs_exist_ok=True)
    dll = site / "espeakng_loader" / "espeak-ng.dll"
    if not dll.is_file():
        sys.exit(f"voice-licenses: no {dll}")
    if b"\0" + ESPEAK_NG.encode() + b"\0" not in dll.read_bytes():
        sys.exit(f"voice-licenses: {dll.name} doesn't say espeak-ng {ESPEAK_NG}: update ESPEAK_NG (and SOURCE.txt's pointer)")
    pa = site / "_sounddevice_data" / "portaudio-binaries" / "libportaudio64bit.dll"
    if not pa.is_file() or f"PortAudio V{PORTAUDIO}".encode() not in pa.read_bytes():
        sys.exit(f"voice-licenses: {pa} isn't PortAudio V{PORTAUDIO}: update PORTAUDIO and licenses\\portaudio\\")

    # Every native binary is someone's: one nobody named stops the bundle.
    native = sorted(p.relative_to(site).as_posix() for p in site.rglob("*") if p.suffix.lower() in (".dll", ".dylib", ".so", ".exe"))
    unknown = [n for n in native if not any(re.match(rx, n) for rx in NATIVE)]
    if unknown:
        sys.exit("voice-licenses: native binaries nobody vouched for (name each in NATIVE, with its licence): " + ", ".join(unknown))
    for d in dists:
        if is_gpl(d):
            if key(d) not in GPL_OK:
                sys.exit(f"voice-licenses: {d.metadata['Name']} is {declared(d)}: a new GPL-family package is a decision (GPL_OK)")
            if any(str(f).lower().endswith((".pyd", ".dll", ".so")) for f in d.files or []):
                sys.exit(f"voice-licenses: {d.metadata['Name']} is GPL and compiled: SOURCE.txt can't call it its own source")
        elif key(d) not in INSIDE and key(d) not in UNDECLARED and (not ACCEPTED.search(declared(d)) or REFUSED.search(
                " ".join([declared(d)] + [c for c in d.metadata.get_all("Classifier") or [] if c.startswith("License ::")]))):
            sys.exit(f"voice-licenses: {d.metadata['Name']} {d.version} declares {declared(d)!r}: not on ACCEPTED, or REFUSED beside it")

    python_licence = bundle / "python" / "LICENSE.txt"
    if not python_licence.is_file():
        sys.exit(f"voice-licenses: no {python_licence} (CPython's embeddable package carries it)")

    # The VC++ runtime in python\: VC_RUNTIME, the v14 one its terms are for, and nothing else of it.
    # No debug C runtime anywhere in the bundle.
    debug = sorted(p.relative_to(bundle).as_posix() for p in bundle.rglob("*") if DEBUG_CRT.match(p.name))
    if debug:
        sys.exit("voice-licenses: a debug C runtime (not redistributable): " + ", ".join(debug))
    py = bundle / "python"
    other = sorted(p.name for p in py.iterdir() if VC_NAME.match(p.name) and p.name.lower() not in VC_RUNTIME)
    if other:
        sys.exit(f"voice-licenses: VC++ runtime DLLs in python\\ that VC_RUNTIME doesn't name: {', '.join(other)}")
    vc = {}
    for n in VC_RUNTIME:
        if not (py / n).is_file():
            sys.exit(f"voice-licenses: no python\\{n} (voice-bundle.ps1 copies the VC++ runtime there)")
        v = file_version(py / n)
        if not v or v[0] != VC_RUNTIME_MAJOR:
            sys.exit(f"voice-licenses: python\\{n} is version {v}, not the v{VC_RUNTIME_MAJOR} runtime licenses\\vc-runtime\\ is for")
        vc[n] = ".".join(map(str, v))
    shutil.copytree(HERE / "voice-licenses" / "vc-runtime", out / "vc-runtime", dirs_exist_ok=True)

    lines = [
        "capri-syn's voice bundle: the licences of what's in it besides capri-syn's own files (LICENSE).",
        "",
        "  python\\           CPython's embeddable package: its licence, and those of what it bundles, are in",
        "                    python\\LICENSE.txt.",
        "  site-packages\\    the packages below, each as its wheel installed it, with the licence texts it",
        "                    carries. Every one is on PyPI at the version shown (https://pypi.org/project/NAME/VERSION/).",
        f"  espeak-ng {ESPEAK_NG}   inside espeakng-loader (its DLL and espeak-ng-data): GPL-3.0-or-later; its",
        "                    licence files are in licenses\\espeak-ng\\, and where its source is, in licenses\\SOURCE.txt.",
        "                    Bill Cox's sonic is compiled into that DLL: Apache-2.0, licenses\\espeak-ng\\sonic-LICENSE.",
        f"  PortAudio {PORTAUDIO}  inside sounddevice (libportaudio64bit.dll): licenses\\portaudio\\LICENSE.txt.",
        "  whisper.cpp       inside pywhispercpp (whisper-*.dll, ggml-*.dll): licenses\\whisper.cpp\\LICENSE.",
        "  Microsoft's Visual C++ runtime (python\\msvcp140.dll, msvcp140_1.dll, vcruntime140.dll, vcruntime140_1.dll;",
        "                    msvcp140-*.dll, vcomp140-*.dll from numpy's and pywhispercpp's wheels): Microsoft's, not",
        "                    capri-syn's, and not under its LICENSE. Distributable Code, redistributed unmodified under the",
        "                    Visual Studio licence (its Distributable List, https://aka.ms/vs/18/redistribution: VC\\Redist).",
        "                    Your use of it is under Microsoft's terms for the runtime, licenses\\vc-runtime\\LICENSE.txt:",
        "                    installing or using this bundle accepts them. Pass it on only inside this bundle, unmodified",
        "                    (never on its own: those terms, 4(e) and (f)), with Microsoft's notices left as they are.",
    ] + textwrap.wrap("In python\\: " + ", ".join(f"{n} {v}" for n, v in vc.items()) + ".", 100,
                      initial_indent=" " * 20, subsequent_indent=" " * 20) + [
        "",
    ]
    no_text = []
    for d in dists:
        name, ver = d.metadata["Name"], d.version
        lines += [RULE, f"{name} {ver}", f"  licence: {declared(d)}"]
        src = source_url(d)
        if src:
            lines.append(f"  source:  {src}")
        lines.append(f"  pypi:    https://pypi.org/project/{name}/{ver}/")
        files = licence_files(d)
        if key(d) in INSIDE:
            lines += ["  inside:  " + INSIDE[key(d)][1][0]] + ["           " + x for x in INSIDE[key(d)][1][1:]]
        long_licence = (d.metadata.get("License") or "").strip()
        if key(d) in UNDECLARED:
            lines.append(f"  (declares none; the licence its wheel carries is {UNDECLARED[key(d)]})")
        if not files and "\n" in long_licence:
            lines += ["", "--- its License field:", long_licence]
        elif not files and APACHE.match(declared(d)):
            no_text.append(f"{name} {ver}")
            lines += ["", "--- Apache-2.0's text (its wheel carries none)", (HERE / "voice-licenses" / "espeak-ng" / "COPYING.APACHE").read_text(encoding="utf-8").rstrip()]
        elif not files and key(d) not in INSIDE:
            no_text.append(f"{name} {ver}")
            lines.append("  (its wheel carries no licence text)")
        for rel, p in files:
            lines += ["", f"--- {rel}", p.read_text(encoding="utf-8", errors="replace").rstrip()]
        lines.append("")
    if no_text:
        lines += [RULE, "Wheels that carry no licence text (their declared licence is above): " + ", ".join(no_text), ""]
    (out / "THIRD-PARTY-LICENSES.txt").write_text("\n".join(lines) + "\n", encoding="utf-8")

    gpl = [d for d in dists if is_gpl(d)]
    version = re.search(r'(?m)^version = "([^"]+)"', (HERE.parents[1] / "Cargo.toml").read_text(encoding="utf-8")).group(1)
    rel = f"https://github.com/HaseebKhalid1507/capri-syn/releases/download/v{version}"
    src = [
        "Where the source of the GPL-licensed software in capri-syn's voice bundle is (GPL-3.0, section 6).",
        "",
        f"espeak-ng {ESPEAK_NG}, GPL-3.0-or-later (licenses\\espeak-ng\\COPYING; its Windows getopt.c is",
        "BSD-2-Clause, COPYING.BSD2; COPYING.APACHE and COPYING.UCD are the other licence files of its",
        "source tree). In object code form here: site-packages\\espeakng_loader\\espeak-ng.dll and its data,",
        f"site-packages\\espeakng_loader\\espeak-ng-data\\, from espeakng-loader {LOADER}'s wheel. espeakng-loader",
        "builds espeak-ng's own source unchanged (its build.sh: CMake with BUILD_SHARED_LIBS=ON,",
        "ENABLE_TESTS=OFF, COMPILE_INTONATIONS=OFF, Release). espeak-ng's CMake (cmake/deps.cmake) fetches",
        f"Bill Cox's sonic at commit {SONIC_COMMIT[:7]} and compiles sonic.c into the DLL (Apache-2.0,",
        "licenses\\espeak-ng\\sonic-LICENSE).",
        "",
        f"Its Corresponding Source, attached to capri-syn's release v{version}, the one this installer came from:",
        f"  {rel}/{ESPEAK_NG_SOURCE}   (espeak-ng {ESPEAK_NG}, commit {ESPEAK_NG_COMMIT})",
        f"  {rel}/{LOADER_SOURCE}   (the scripts that built the DLL: build.sh, .github/workflows/)",
        f"  {rel}/{SONIC_SOURCE}   (sonic {SONIC_COMMIT[:7]}: espeak-ng's CMake fetches it and compiles sonic.c into the DLL)",
        "The same, upstream:",
        f"  https://github.com/espeak-ng/espeak-ng/tree/{ESPEAK_NG}",
        f"  https://github.com/thewh1teagle/espeakng-loader/tree/{LOADER_COMMIT}",
        f"  https://github.com/waywardgeek/sonic/tree/{SONIC_COMMIT}",
        "",
    ]
    if gpl:
        src.append("GPL-licensed Python packages, here as their own source (the .py files under site-packages\\):")
        for d in gpl:
            src.append(f"  {d.metadata['Name']} {d.version} ({declared(d)}): https://pypi.org/project/{d.metadata['Name']}/{d.version}/#files")
        src.append("")
    (out / "SOURCE.txt").write_text("\n".join(src).rstrip("\n") + "\n", encoding="utf-8")

    print(f"voice licences: {len(dists)} packages, {sum(1 for d in dists if licence_files(d))} with their licence texts"
          + (f"; none in the wheel: {', '.join(no_text)}" if no_text else "")
          + f"; GPL: espeak-ng {ESPEAK_NG}" + "".join(f", {d.metadata['Name']} {d.version}" for d in gpl))


if __name__ == "__main__":
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    main(Path(sys.argv[1]).resolve())

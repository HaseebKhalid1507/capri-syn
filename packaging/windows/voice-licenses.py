"""The voice bundle's licences (voice-bundle.ps1 runs it once the wheels are in).

    python voice-licenses.py BUNDLE

writes BUNDLE/licenses/:
  THIRD-PARTY-LICENSES.txt  every package in BUNDLE/site-packages: its version, its declared licence,
                            where its source is, and the licence texts its wheel carries
  SOURCE.txt                where the source of the GPL-licensed parts is (GPL-3.0 section 6)
  espeak-ng/, whisper.cpp/  the licence files of what two wheels carry inside them without one
                            (packaging/windows/voice-licenses/)

espeak-ng's DLL comes inside espeakng-loader's wheel, whisper.cpp's and ggml's DLLs inside
pywhispercpp's; neither wheel has their licence text. Both wheels are pinned
(tools/capri-voice-win-requirements.txt) and checked here against the versions INSIDE was written
for, and espeak-ng's DLL against the version SOURCE.txt names: a new wheel can't ship under a stale
notice.
"""

import re
import shutil
import sys
from importlib import metadata
from pathlib import Path

# The espeak-ng in espeakng-loader 0.2.4's Windows wheel (tools/capri-voice-win-requirements.txt pins
# it): the version its DLL reports, and that tag's commit in espeak-ng's repository.
ESPEAK_NG = "1.52.0"
ESPEAK_NG_COMMIT = "4870adfa25b1a32b4361592f1be8a40337c58d6c"
LOADER = "0.2.4"

# What a wheel carries inside it without its licence text: (the wheel's version these lines were
# checked for, the lines, the folder of licence files under voice-licenses/ that goes with them).
INSIDE = {
    "espeakng-loader": (LOADER, [
        f"espeak-ng {ESPEAK_NG} (espeak-ng.dll, espeak-ng-data): GPL-3.0-or-later; its licence files are in",
        "licenses\\espeak-ng\\, and where its source is, in licenses\\SOURCE.txt",
        "the loader itself (__init__.py): MIT, its repository's LICENSE (none in the wheel):",
        "https://github.com/thewh1teagle/espeakng-loader",
    ], "espeak-ng"),
    # pywhispercpp v1.5.1's whisper.cpp submodule: ggml-org/whisper.cpp at f24588a272ae.
    "pywhispercpp": ("1.5.1", [
        "whisper.cpp and ggml (whisper-*.dll, ggml-*.dll; whisper.cpp at f24588a272ae): MIT, licenses\\whisper.cpp\\LICENSE",
        "msvcp140-*.dll and vcomp140-*.dll: Microsoft's Visual C++ runtime, as its wheel ships them",
        "(Distributable Code under Microsoft's Visual Studio licence terms)",
    ], "whisper.cpp"),
}

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


def key(dist):
    return dist.metadata["Name"].lower().replace("_", "-")


def is_gpl(dist):
    text = " ".join([declared(dist), dist.metadata.get("License") or ""] + (dist.metadata.get_all("Classifier") or []))
    return re.search(r"\bGPL|General Public License", text) is not None


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

    python_licence = bundle / "python" / "LICENSE.txt"
    if not python_licence.is_file():
        sys.exit(f"voice-licenses: no {python_licence} (CPython's embeddable package carries it)")

    lines = [
        "capri-syn's voice bundle: the licences of what's in it besides capri-syn's own files (LICENSE).",
        "",
        "  python\\           CPython's embeddable package: its licence, and those of what it bundles, are in",
        "                    python\\LICENSE.txt.",
        "  site-packages\\    the packages below, each as its wheel installed it, with the licence texts it",
        "                    carries. Every one is on PyPI at the version shown (https://pypi.org/project/NAME/VERSION/).",
        f"  espeak-ng {ESPEAK_NG}   inside espeakng-loader (its DLL and espeak-ng-data): GPL-3.0-or-later; its",
        "                    licence files are in licenses\\espeak-ng\\, and where its source is, in licenses\\SOURCE.txt.",
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
    src = [
        "Where the source of the GPL-licensed software in capri-syn's voice bundle is (GPL-3.0, section 6).",
        "",
        f"espeak-ng {ESPEAK_NG}, GPL-3.0-or-later (licenses\\espeak-ng\\COPYING; its Windows getopt.c is",
        "BSD-2-Clause, COPYING.BSD2; COPYING.APACHE and COPYING.UCD are the other licence files of its",
        "source tree). In object code form here: site-packages\\espeakng_loader\\espeak-ng.dll and its data,",
        f"site-packages\\espeakng_loader\\espeak-ng-data\\, from espeakng-loader {LOADER}'s wheel. espeakng-loader",
        "builds espeak-ng's own source unchanged (its build.sh: CMake with BUILD_SHARED_LIBS=ON,",
        "ENABLE_TESTS=OFF, COMPILE_INTONATIONS=OFF, Release): https://github.com/thewh1teagle/espeakng-loader",
        "",
        "Its Corresponding Source, espeak-ng's release " + ESPEAK_NG + ":",
        f"  https://github.com/espeak-ng/espeak-ng/tree/{ESPEAK_NG} (commit {ESPEAK_NG_COMMIT})",
        f"  https://github.com/espeak-ng/espeak-ng/archive/refs/tags/{ESPEAK_NG}.tar.gz",
        "",
    ]
    if gpl:
        src.append("GPL-licensed Python packages, here as their own source (the .py files under site-packages\\):")
        for d in gpl:
            src.append(f"  {d.metadata['Name']} {d.version} ({declared(d)}): https://pypi.org/project/{d.metadata['Name']}/{d.version}/#files")
        src.append("")
    src += [
        "If a link above stops working, open an issue at https://github.com/HaseebKhalid1507/capri-syn/issues",
        "and you'll be sent the same source.",
    ]
    (out / "SOURCE.txt").write_text("\n".join(src) + "\n", encoding="utf-8")

    print(f"voice licences: {len(dists)} packages, {sum(1 for d in dists if licence_files(d))} with their licence texts"
          + (f"; none in the wheel: {', '.join(no_text)}" if no_text else "")
          + f"; GPL: espeak-ng {ESPEAK_NG}" + "".join(f", {d.metadata['Name']} {d.version}" for d in gpl))


if __name__ == "__main__":
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    main(Path(sys.argv[1]).resolve())

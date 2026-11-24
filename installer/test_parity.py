#!/usr/bin/env python3
"""The two installers are one installer in two shells: install.sh (Linux) and install.ps1 (Windows)
take the same flags, run the same steps in the same order, and read their pins from the one
installer/manifest. This checks that they still do (CI's `installer` job).

python3 installer/test_parity.py   (stdlib only)"""
import re, unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SH = (ROOT / "install.sh").read_text()
PS = (ROOT / "install.ps1").read_text(encoding="utf-8")
MANIFEST = (ROOT / "installer/manifest").read_text()
# Windows' install steps, shared with her installer (where they are, the Synaps step is).
STEPS = ROOT / "packaging/windows/steps.ps1"
# Her provider table: the brains both installers list (`synaps-avatar --providers`).
PROVIDER = (ROOT / "src/provider.rs").read_text(encoding="utf-8")


def block(text, start, end):
    i = text.index(start)
    return text[i:text.index(end, i + len(start))]


def code(text):
    """Without its whole-line comments (they may say what it never does)."""
    return re.sub(r"(?m)^\s*#.*$", "", text)


def flags(usage):
    return sorted(set(re.findall(r"--[a-z][a-z-]*", usage)) - {"--help"})


class Parity(unittest.TestCase):
    def test_the_same_flags(self):
        sh = flags(block(SH, "usage: install.sh", "EOF"))
        ps = flags(block(PS, "usage: install.ps1", "'@"))
        self.assertEqual(sh, ps)
        self.assertEqual(sh, ["--allow-root", "--artifact", "--brain", "--dry-run", "--from-source", "--no-autostart",
                              "--no-local-art", "--no-upscale", "--prefix", "--purge", "--uninstall", "--upscale", "--voice",
                              "--watch", "--with-local-art", "--yes"])

    def test_the_full_pack_is_the_default(self):
        # mochi's art on every install path unless asked not to (--with-local-art, its old opt-in, a no-op);
        # lachy's x2 strips as the manifest's pack_upscale says, which each usage and her installer's box say too.
        steps = STEPS.read_text(encoding="utf-8")
        iss = (ROOT / "packaging/windows/synaps-avatar.iss").read_text(encoding="utf-8")
        sh_args = block(SH, "args() {", "\n}\n")
        self.assertRegex(sh_args, r"\bLOCAL_ART=1 UPSCALE=''")
        self.assertIn("--with-local-art) LOCAL_ART=1", sh_args)
        self.assertIn("--no-local-art) LOCAL_ART=0", sh_args)
        self.assertIn("$script:LocalArt = $true; $script:Upscale = ''", block(PS, "function Read-Flags", "\n}\n"))
        self.assertIn("$mochi = -not $NoLocalArt", block(steps, "function Build-Pack", "\n}\n"))
        self.assertIn("MochiBox.Checked := not ParamOff('LOCALART');", iss)
        up = re.search(r"^pack_upscale=(on|off)$", MANIFEST, re.M).group(1)
        for name, usage in (("install.sh", block(SH, "usage: install.sh", "EOF")), ("install.ps1", block(PS, "usage: install.ps1", "'@"))):
            self.assertIn(f"(the default: {up})", usage, f"{name}'s usage says the x2 default the manifest does")
        self.assertIn("UpscaleBox.Checked := " + ("not ParamOff('UPSCALE');" if up == "on" else "ParamOn('UPSCALE');"), iss,
                      "her installer's x2 box starts as the manifest's pack_upscale")
        for name, text in (("install.sh", SH), ("install.ps1", PS), ("steps.ps1", steps), ("synaps-avatar.iss", iss),
                           ("make-pack", (ROOT / "tools/make-pack").read_text())):
            self.assertNotIn("PERSONAL USE ONLY", text, f"{name}: the art's terms are its artists' (NOTICE.md), not a warning of ours")

    def test_the_installers_carry_what_make_pack_reads(self):
        # Every pin and helper make-pack reads for the full pack, in the Linux layout and her Windows installer.
        stage = (ROOT / "installer/stage.sh").read_text()
        iss = (ROOT / "packaging/windows/synaps-avatar.iss").read_text(encoding="utf-8")
        for f in ("mochi_hair.py", "upscale_art.py"):
            self.assertIn(f'"$src/tools/{f}"', stage)
        self.assertIn('"$src"/tools/*.sha256', stage)
        for f in ("make-pack", "capricore-*.sha256", "lachy-desktop-*.sha256", "mochi-desktop-*.sha256", "lachy-x2.sha256",
                  "mochi_hair.py", "upscale_art.py", "desktop-art.json", "capricore-outfit.json"):
            self.assertTrue(f'Source: "..\\..\\tools\\{f}"; DestDir: "{{app}}\\steps\\tools"' in iss, f"her installer has no tools\\{f}")

    def test_every_flag_is_parsed(self):
        sh_args = block(SH, "args() {", "\n}\n")
        ps_args = block(PS, "function Read-Flags", "\n}\n")
        for f in flags(block(SH, "usage: install.sh", "EOF")):
            self.assertIn(f, sh_args, f"install.sh doesn't parse {f}")
            self.assertIn(f"'{f[2:].replace('-', '')}'", ps_args, f"install.ps1 doesn't parse {f}")

    def test_powershell_spellings_are_listed(self):
        usage = block(PS, "usage: install.ps1", "'@")
        for f in flags(block(SH, "usage: install.sh", "EOF")):
            ps = "-" + "".join(w.capitalize() for w in f[2:].split("-"))
            self.assertIn(ps, usage, f"install.ps1's usage doesn't list {ps}")

    def test_the_same_steps_in_the_same_order(self):
        # The functions each main calls, in order, that open with a step header.
        def steps(main, define, header):
            names = []
            for call in re.findall(r"^\s+([A-Za-z_][\w-]*)$", main, re.M):
                m = re.search(define.format(re.escape(call)) + r"\n((?:.*\n){1,3})", SH + PS, re.M)
                h = m and re.search(header, m.group(1))
                if h:
                    names.append(h.group(1))
            return names
        sh = steps(block(SH, "main() {", "\n}\n"), r"^{}\(\) {{", r'^\s+step "([^"]+)"')
        ps = steps(block(PS, "function Invoke-CapriInstall", "\n}\n"), r"^function {} {{", r"^\s+Write-Step '([^']+)'")
        self.assertEqual(len(sh), 7, sh)
        self.assertEqual(sh, ps)
        self.assertIn("STEPS=7", SH)
        self.assertIn("$Steps = 7", PS)

    def test_every_manifest_key_they_read_is_there(self):
        keys = set(re.findall(r"^([a-z0-9_]+)=", MANIFEST, re.M))
        used_sh = set(re.findall(r"\bM_([a-z0-9_]+)", SH)) - {"k", "base_apt"} | {"base_apt"}
        used_sh = {k for k in used_sh if not k.endswith("_")}
        used_ps = set(re.findall(r"\$script:M\.([a-z0-9_]+)", PS)) | set(re.findall(r"\$m\[\$k\].*?'([a-z0-9_]+)'", PS))
        for k in sorted(used_sh | used_ps):
            self.assertIn(k, keys, f"installer/manifest has no {k}")
        # The per-package-manager lists install.sh builds names for.
        for kind in ("base", "build", "run", "vulkan", "pipewire"):
            for pm in ("apt", "pacman", "dnf", "zypper"):
                self.assertIn(f"{kind}_{pm}", keys)

    def test_synaps_is_the_pin_never_latest(self):
        # Her client speaks synaps_protocol exactly: Synaps is the manifest's pin and its hash, never
        # GitHub's latest (a newer one may speak another protocol).
        body = block(SH, "step_synaps() {", "\n}\n")
        self.assertNotRegex(code(body), r"api\.github\.com|releases/latest")
        self.assertIn("M_synaps_pin", body)
        self.assertIn("M_synaps_installer_sh_sha256", body)

    @unittest.skipUnless(STEPS.exists(), "install.ps1 takes the latest Synaps until its steps are steps.ps1's")
    def test_synaps_is_the_pin_never_latest_on_windows(self):
        steps = block(STEPS.read_text(encoding="utf-8"), "function Install-Synaps", "\n}\n")
        self.assertNotRegex(code(steps), r"api\.github\.com|releases/latest")
        self.assertIn("synaps_pin", steps)
        self.assertIn("synaps_installer_ps1_sha256", steps)
        self.assertNotRegex(code(block(PS, "function Install-Synaps", "\n}\n")), r"api\.github\.com|releases/latest")

    def test_her_voice_is_her_own_release_never_latest(self):
        voice = code(block(STEPS.read_text(encoding="utf-8"), "function Find-Voice", "\n}\n"))
        self.assertNotIn("releases/latest", voice)
        self.assertIn("releases/download/$Release/", voice)
        # Both of her Windows installers name the release: her installer its own version, install.ps1
        # the one in her Apps entry (the setup it just ran).
        iss = (ROOT / "packaging/windows/synaps-avatar.iss").read_text(encoding="utf-8")
        self.assertIn("'mics voice-check -Release v{#AppVersion}'", iss)
        self.assertIn("'voice -Release v{#AppVersion} ", iss)
        extras = code(block(PS, "function Install-Extras", "\n}\n"))
        self.assertIn("@('-Release', \"v$ver\")", extras)
        self.assertEqual(extras.count("$herRelease"), 3)

    def test_her_voice_asks_for_one_python_range(self):
        # kokoro-onnx 0.6.1 wants Python >=3.10,<3.14: install.sh's voice_python and the setup it runs
        # (tools/capri-voice-setup) look for the same interpreters (python3 first), accept the same versions (CPython only) and say the
        # same range; the Windows bundle's CPython is inside it.
        setup = (ROOT / "tools/capri-voice-setup").read_text(encoding="utf-8")
        for what, text in (("install.sh", SH), ("tools/capri-voice-setup", setup)):
            self.assertIn("python3 python3.13 python3.12 python3.11 python3.10", text, what)
            self.assertIn("-I -S -c", text, what)
            self.assertIn('sys.implementation.name == "cpython"', text, what)
            self.assertIn("3.10 | 3.11 | 3.12 | 3.13", text, what)
            self.assertIn("Python 3.10–3.13", text, what)
        embed = int(re.search(r"^python_embed=3\.(\d+)\.", MANIFEST, re.M).group(1))
        self.assertTrue(10 <= embed <= 13, "the Windows bundle's CPython is outside kokoro-onnx's range")

    def test_the_release_builds_the_assets_the_installers_fetch(self):
        # install.sh and install.ps1 fetch releases/latest/download/<asset_*> (installer/manifest); a
        # rename on one side only 404s every install the day the release is published.
        rel = (ROOT / ".github/workflows/release.yml").read_text(encoding="utf-8")
        assets = dict(re.findall(r"^(asset_\w+)=(\S+)$", MANIFEST, re.M))
        self.assertEqual(set(assets), {"asset_linux", "asset_windows", "asset_windows_voice"})
        assemble = block(rel, "  assemble:", "  publish:")
        for key, name in assets.items():
            self.assertTrue(name.removesuffix(".tar.gz") in code(rel), f"release.yml doesn't build {key} ({name})")
            self.assertIn(key, assemble, f"release.yml's assemble doesn't check {key} is there")
        self.assertFalse("--generate-notes" in code(rel), "release.yml's notes are CHANGELOG.md's, never --generate-notes")
        self.assertTrue("--notes-file notes/notes.md" in rel, "release.yml publishes notes/notes.md")

    def test_the_voice_licences_name_the_wheels_it_bundles(self):
        # voice-licenses.py's notes on what two wheels carry inside them are written for one version
        # of each: the requirements pin exactly those.
        req = (ROOT / "tools/capri-voice-win-requirements.txt").read_text(encoding="utf-8")
        vl = (ROOT / "packaging/windows/voice-licenses.py").read_text(encoding="utf-8")
        loader = re.search(r'^LOADER = "([^"]+)"', vl, re.M).group(1)
        self.assertRegex(req, rf"(?m)^espeakng-loader=={re.escape(loader)}\b")
        whisper = re.search(r'"pywhispercpp": \("([^"]+)"', vl).group(1)
        self.assertRegex(req, rf"(?m)^pywhispercpp=={re.escape(whisper)}\b")
        sounddevice = re.search(r'"sounddevice": \("([^"]+)"', vl).group(1)
        self.assertRegex(req, rf"(?m)^sounddevice=={re.escape(sounddevice)}\b")

    def test_the_release_carries_espeak_ngs_source(self):
        # GPL-3.0 6(d): espeak-ng's Corresponding Source beside the voice setup, as assets of the same
        # release (voice-licenses.py's SOURCE.txt points there), summed and attested with the rest.
        rel = (ROOT / ".github/workflows/release.yml").read_text(encoding="utf-8")
        vl = (ROOT / "packaging/windows/voice-licenses.py").read_text(encoding="utf-8")
        ver = re.search(r'^ESPEAK_NG = "([^"]+)"', vl, re.M).group(1)
        loader = re.search(r'^LOADER = "([^"]+)"', vl, re.M).group(1)
        sonic = re.search(r'^SONIC_COMMIT = "([^"]+)"', vl, re.M).group(1)
        assemble = code(block(rel, "  assemble:", "  publish:"))
        sums = assemble.index("> SHA256SUMS")
        # Fetched by the versions voice-licenses.py names, checked against SHA-256s pinned for these
        # file names: a version bump on one side only fails here, and in the job.
        for asset in (f"release/espeak-ng-{ver}.tar.gz", f"release/espeakng-loader-{loader}-build.tar.gz", f"release/sonic-{sonic[:7]}.tar.gz"):
            self.assertRegex(assemble[:sums], rf"[0-9a-f]{{64}}  {re.escape(asset)}", f"no pinned SHA-256 for {asset} before SHA256SUMS")
        for name in ("ESPEAK_NG", "LOADER", "LOADER_COMMIT", "SONIC_COMMIT"):
            self.assertTrue(f"v {name}" in assemble, f"assemble doesn't read {name} from voice-licenses.py")
        self.assertTrue("sha256sum -c -" in assemble[:sums])

    def test_install_ps1_is_ascii(self):
        # Windows PowerShell 5.1 reads a script without a BOM as ANSI.
        bad = [(i + 1, l) for i, l in enumerate(PS.splitlines()) if not l.isascii()]
        self.assertEqual(bad, [])

    def test_both_run_whole_or_not_at_all(self):
        # A cut-off download must not run half: install.sh is all functions until its last line;
        # install.ps1 is one module (its last statement runs it), piped into the block that turns a
        # stop into exit 1; a cut inside either leaves a brace open, so nothing parses.
        self.assertEqual(SH.rstrip().splitlines()[-1], 'main "$@"')
        tail = PS.rstrip().splitlines()
        self.assertEqual(tail[-1], "}")
        module_end = tail.index("} | ForEach-Object {")
        self.assertIn("try { Invoke-CapriInstall $CliArgs }", tail[module_end - 6:module_end])

    def test_install_ps1_leaves_nothing_in_the_callers_scope(self):
        # `irm | iex` runs it in his session's scope: above the module only comments, the analyzer's
        # attributes and param(); the preferences are set inside Invoke-CapriInstall.
        top = PS[:PS.index("New-Module -Name capri-install")]
        code = [l for l in top.splitlines() if l.strip() and not l.lstrip().startswith(("#", "[Diagnostics."))]
        self.assertEqual(code, ["param()"])
        self.assertIn("-ArgumentList (, [string[]]$args)", PS)
        self.assertIn("Export-ModuleMember -Function @()", PS)
        body = block(PS, "function Invoke-CapriInstall", "\n}\n")
        for pref in ("$ErrorActionPreference = 'Stop'", "$ProgressPreference = 'SilentlyContinue'"):
            self.assertIn(pref, body)

    def test_both_read_flags_from_the_environment(self):
        # `irm | iex` can't pass flags: both take CAPRI_INSTALL_FLAGS when given none.
        self.assertIn("CAPRI_INSTALL_FLAGS", block(SH, "main() {", "\n}\n"))
        self.assertIn("CAPRI_INSTALL_FLAGS", block(PS, "function Invoke-CapriInstall", "\n}\n"))

    # The daemon step's three stops, said the same way on both (her client speaks synaps_protocol
    # exactly): a daemon that doesn't say its protocol; one that speaks another right after this run
    # installed or upgraded Synaps (an older one still running: installing the pin again won't help);
    # and one that speaks another anyway.
    DAEMON_STOPS = ("doesn't say which protocol it speaks", "is installed, but the daemon still running is", "(exact match)")

    def test_a_daemon_that_isnt_hers_stops_windows(self):
        steps = code(block(STEPS.read_text(encoding="utf-8"), "function Start-Daemon {", "\n}\n"))
        for why in self.DAEMON_STOPS:
            line = next((l for l in steps.splitlines() if why in l), "")
            self.assertIn("Stop-Step", line, f"steps.ps1's daemon step doesn't stop on: {why}")
        # An unknown protocol is checked first, never let through as "running (protocol ?)".
        self.assertLess(steps.index("-eq '?'"), steps.index("Result \"daemon: running"))
        self.assertIn("$script:Fresh", steps)
        # Both of her Windows installers tell it when they just installed Synaps, and install.ps1
        # stops on it as install.sh does.
        self.assertIn("'-Fresh'", block(PS, "function Start-Daemon", "\n}\n"))
        self.assertIn("Stop-Install", block(PS, "function Start-Daemon", "\n}\n"))
        self.assertIn("daemon -Fresh", (ROOT / "packaging/windows/synaps-avatar.iss").read_text(encoding="utf-8"))

    def test_a_daemon_that_isnt_hers_stops_linux_too(self):
        sh = code(block(SH, "step_daemon() {", "\n}\n"))
        if self.DAEMON_STOPS[0] not in sh:
            self.skipTest("install.sh's daemon step says these once #98 is in")
        for why in self.DAEMON_STOPS:
            line = next((l for l in sh.splitlines() if why in l), "")
            self.assertIn("die ", line, f"install.sh's daemon step doesn't stop on: {why}")

    # `synaps daemon status` as Synaps 0.11 and 0.12 print it (src/cmd/daemon.rs `status`): "running"
    # only when the daemon answered this synaps' ping, exit 0. One on another protocol refuses the
    # ping: "running (not answering)", exit 1, its protocol from daemon.json. Both installers must
    # read that as a daemon that runs (then stop on its protocol), whatever the exit code.
    STATUS = {
        "hers": ("daemon: running\nsocket: /run/user/1000/synaps/daemon.sock\npid: 4242  version: 0.12.0  protocol: 3  started: 2026-11-19T00:00:00Z\nuptime: 5s  sessions: 1\n", 0, True, "3"),
        "another protocol": ("daemon: running (not answering)\nsocket: /run/user/1000/synaps/daemon.sock\npid: 4242  version: 0.10.2  protocol: 2  started: 2026-11-19T00:00:00Z\n", 1, True, "2"),
        "no daemon.json": ("daemon: running\nsocket: /run/user/1000/synaps/daemon.sock\nuptime: 5s  sessions: 1\n", 0, True, None),
        "not running": ("daemon: not running (auto-starts on first --attach)\nsocket: /run/user/1000/synaps/daemon.sock\n", 1, False, None),
        "stale": ("daemon: stale (pid dead)\nsocket: /run/user/1000/synaps/daemon.sock\npid: 4242  version: 0.12.0  protocol: 3  started: 2026-11-19T00:00:00Z\n", 1, False, "3"),
    }

    def test_both_read_daemon_status_the_same(self):
        ps = block(STEPS.read_text(encoding="utf-8"), "function Get-DaemonStatus", "\n}\n")
        m = re.search(r"\$alive = \$t -match '([^']+)'", ps)
        self.assertIsNotNone(m, "steps.ps1's Get-DaemonStatus has no Alive (a daemon that runs, whatever the exit code)")
        ps_alive = m.group(1)
        ps_proto = re.search(r"\$t -match '(protocol: [^']+)'", ps).group(1)
        self.assertNotIn("LASTEXITCODE", next(l for l in ps.splitlines() if "$alive =" in l), "Alive never hangs on the exit code")
        sh = code(block(SH, "step_daemon() {", "\n}\n"))
        sh_alive = re.search(r"""grep -q '(daemon: [^']+)'""", sh).group(1)
        sh_proto = re.search(r"""grep -o '(protocol: [^']+)'""", sh).group(1)
        for name, (text, rc, alive, proto) in self.STATUS.items():
            with self.subTest(name):
                # PowerShell's -match: .NET, case-insensitive; grep -q: a line holds it.
                self.assertEqual(bool(re.search(ps_alive, text, re.I | re.M)), alive, f"steps.ps1, exit {rc}")
                self.assertEqual(bool(re.search(sh_alive, text)), alive, f"install.sh, exit {rc}")
                m = re.search(ps_proto, text, re.I)
                self.assertEqual(m.group(1) if m else None, proto, "steps.ps1's protocol")
                m = re.search(sh_proto, text)
                self.assertEqual(m.group(0).split()[-1] if m and m.group(0).split()[-1].isdigit() else None, proto, "install.sh's protocol")

    def test_windows_checks_the_protocol_of_any_daemon_that_runs(self):
        steps = code(block(STEPS.read_text(encoding="utf-8"), "function Start-Daemon {", "\n}\n"))
        # After the wait: "not running" only when none runs; the protocol stops come before "running",
        # and a hung one of hers (Alive, not Up) is never "running".
        self.assertIn("if (-not $st.Alive) {", steps)
        self.assertLess(steps.index("if (-not $st.Alive) {"), steps.index("-eq '?'"))
        self.assertLess(steps.index("$script:Fresh"), steps.rindex("if (-not $st.Up) {"))
        self.assertLess(steps.rindex("if (-not $st.Up) {"), steps.index("Result \"daemon: running"))
        # Step 1's upgrade warning: any daemon the old Synaps left, answering or not.
        self.assertIn("(Get-DaemonStatus $mine).Alive", STEPS.read_text(encoding="utf-8"))

    def test_both_pickers_show_her_tables_hint(self):
        # --providers' last field (src/provider.rs' Provider::hint: ChatGPT's, that any account,
        # a free one too, signs her in), beside an account's name in both pickers and setup's.
        # Neither keeps a copy of it: what her table says is what they show.
        steps = STEPS.read_text(encoding="utf-8")
        iss = (ROOT / "packaging/windows/synaps-avatar.iss").read_text(encoding="utf-8")
        self.assertIn('"|" $7', block(code(SH), "brains_here() {", "\n}\n"))
        self.assertIn("read -r a b c d e", block(code(SH), "pick_row() {", "\n}\n"))
        self.assertIn("Hint = $(if ($f.Count -ge 7) { $f[6] }", code(block(steps, "function Get-Brains", "\n}\n")))
        self.assertIn("|$($b.Hint)\"", code(block(steps, "function Show-Brains", "\n}\n")))
        self.assertIn("Hint = $(if ($f.Count -gt 5) { $f[5] }", code(PS))
        self.assertIn("$x.Hint", code(block(PS, "function Invoke-Brain", "\n}\n")))
        self.assertIn("Field(L, 6)", block(iss, "procedure FillModelPage", "\nend;\n"))
        hint = re.search(r'"openai-codex" => "([^"]+)"', block(PROVIDER, "pub fn hint(", "\n    }\n")).group(1)
        for name, text in (("install.sh", SH), ("install.ps1", PS), ("steps.ps1", steps), ("synaps-avatar.iss", iss)):
            self.assertNotIn(hint, text, f"{name} keeps a copy of ChatGPT's hint")

    def test_the_readme_names_chatgpts_default(self):
        # "Free" is true on the model a ChatGPT sign-in puts her on (provider::CHATGPT_DEFAULT, the
        # head of its list, her installers' --set session.model=): the README names that one, so a
        # swap of it is a swap of the README's words too.
        model, label = re.search(r'pub const CHATGPT_DEFAULT: \(&str, &str\) = \("([^"]+)", "([^"]+)"\);', PROVIDER).groups()
        self.assertTrue(model.startswith("openai-codex/"), model)
        readme = (ROOT / "README.md").read_text(encoding="utf-8")
        free = " ".join(block(readme, "**Free:**", "\n\n").split())
        self.assertIn(label.lower(), free.lower(), f"README's Free line names ChatGPT's default, {label}")


if __name__ == "__main__":
    unittest.main()

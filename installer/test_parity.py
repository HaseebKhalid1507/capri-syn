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
        self.assertEqual(sh, ["--allow-root", "--artifact", "--dry-run", "--from-source", "--no-autostart", "--prefix",
                              "--purge", "--uninstall", "--voice", "--watch", "--with-local-art", "--yes"])

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
        for kind in ("base", "build", "run", "vulkan"):
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


if __name__ == "__main__":
    unittest.main()

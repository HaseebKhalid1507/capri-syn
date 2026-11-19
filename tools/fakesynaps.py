"""A stand-in `synaps login --provider <id>` for the shots: GitHub Copilot's device login as Synaps
0.12 says it (its code and page on stderr, coloured), no browser. FAKE_SIGNIN_AFTER seconds in, its
login goes into auth.json (SYNAPS_BASE_DIR) and it says so; either way it ends only once its stdin
closes, as Synaps 0.12's does.
"""
import json
import os
import sys
import time

sys.stderr.reconfigure(encoding="utf-8")  # her reader takes UTF-8 lines (a pipe on Windows isn't)
args = sys.argv[1:]
provider = args[args.index("--provider") + 1] if "--provider" in args else ""
err = sys.stderr
print("\n\x1b[1mGitHub Copilot device login\x1b[0m", file=err)
print("  Enter code: \x1b[36mFAKE-C0DE\x1b[0m", file=err)
print("  At:         \x1b[36mhttps://github.com/login/device\x1b[0m\n", file=err, flush=True)
after = float(os.environ.get("FAKE_SIGNIN_AFTER", "0") or 0)
if after > 0:
    time.sleep(after)
    path = os.path.join(os.environ.get("SYNAPS_BASE_DIR", "."), "auth.json")
    try:
        with open(path, encoding="utf-8-sig") as f:
            auth = json.load(f)
    except (OSError, ValueError):
        auth = {}
    auth[provider] = {"type": "oauth"}
    with open(path, "w", encoding="utf-8") as f:
        json.dump(auth, f)
    print("\n\x1b[32m✓ Login successful!\x1b[0m", file=err, flush=True)
sys.stdin.read()

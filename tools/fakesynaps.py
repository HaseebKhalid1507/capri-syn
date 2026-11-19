"""A stand-in `synaps login --provider <login>` for the shots, saying what Synaps 0.12 says on stderr
(coloured, as `src/cmd/login.rs` and its providers print it), no browser:

  - a device login (github-copilot, kimi-code, muse): its code and page;
  - a PKCE login (claude, openai-codex, google-gemini): its sign-in page, whole, on the line after
    "If the browser didn't open, visit this URL:";
  - an API key (any other): one line read off its stdin; FAKE_KEY_EXPECT is the key it takes
    (never printed, never written: auth.json gets the key's slot, not the key).

FAKE_SIGNIN_AFTER seconds in, an account's login goes into auth.json (SYNAPS_BASE_DIR) and it says
so, unless its login is in FAKE_SIGNIN_NEVER (comma-separated); either way it ends only once its
stdin closes, as Synaps 0.12's does. FAKE_SYNAPS_PIDS: a file its pid is appended to (the shots
check it's gone with her).
"""
import json
import os
import sys
import time

sys.stderr.reconfigure(encoding="utf-8")  # her reader takes UTF-8 lines (a pipe on Windows isn't)
args = sys.argv[1:]
login = args[args.index("--provider") + 1] if "--provider" in args else ""
provider = {"claude": "anthropic"}.get(login, login)
err = sys.stderr
pids = os.environ.get("FAKE_SYNAPS_PIDS")
if pids:
    with open(pids, "a", encoding="utf-8") as f:
        f.write(f"{os.getpid()}\n")

PKCE = {
    "anthropic": "https://claude.ai/oauth/authorize?code=true&client_id=FAKE&response_type=code&redirect_uri=http%3A%2F%2Flocalhost%3A54545%2Fcallback&state=FAKE",
    "openai-codex": "https://auth.openai.com/oauth/authorize?response_type=code&client_id=FAKE&redirect_uri=http%3A%2F%2Flocalhost%3A1455%2Fauth%2Fcallback&state=FAKE",
    "google-gemini": "https://accounts.google.com/o/oauth2/v2/auth?client_id=FAKE&response_type=code&state=FAKE",
}
DEVICE = {
    "github-copilot": ("GitHub Copilot", "Enter code:", "FAKE-C0DE", "https://github.com/login/device"),
    "kimi-code": ("Kimi Code", "Confirm code:", "FAKE-K1MI", "https://www.kimi.com/code/authorize_device?user_code=FAKE-K1MI"),
    "muse": ("Meta Muse", "Enter code:", "FAKE-MU5E", "https://www.meta.ai/device"),
}


def save(entry):
    path = os.path.join(os.environ.get("SYNAPS_BASE_DIR", "."), "auth.json")
    try:
        with open(path, encoding="utf-8-sig") as f:
            auth = json.load(f)
    except (OSError, ValueError):
        auth = {}
    auth[provider] = entry
    with open(path, "w", encoding="utf-8") as f:
        json.dump(auth, f)


if provider in PKCE:
    print("\n\x1b[1mOpening browser to sign in...\x1b[0m\n", file=err)
    print("\x1b[2mIf the browser didn't open, visit this URL:\x1b[0m", file=err)
    print(f"\x1b[36m{PKCE[provider]}\x1b[0m\n", file=err)
    print("\x1b[2mOr paste the redirect URL here (must include `state`):\x1b[0m", file=err, flush=True)
elif provider in DEVICE:
    name, ask, code, page = DEVICE[provider]
    print(f"\n\x1b[1m{name} device login\x1b[0m", file=err)
    print(f"  {ask} \x1b[36m{code}\x1b[0m", file=err)
    print(f"  At:         \x1b[36m{page}\x1b[0m\n", file=err, flush=True)
else:
    print(f"\nPaste your {provider} API key.", file=err)
    print(f"provider.{provider}: ", end="", file=err, flush=True)
    key = sys.stdin.readline().strip()
    if key and key == os.environ.get("FAKE_KEY_EXPECT", key):
        save({"type": "api_key", "key": "(set by the stand-in)"})
        print("\n\x1b[32m✓ API key saved!\x1b[0m", file=err, flush=True)
        sys.exit(0)
    print("\n\x1b[31m✗ Login failed: not the key it was given\x1b[0m", file=err, flush=True)
    sys.exit(1)

after = float(os.environ.get("FAKE_SIGNIN_AFTER", "0") or 0)
never = [x.strip() for x in os.environ.get("FAKE_SIGNIN_NEVER", "").split(",") if x.strip()]
if after > 0 and login not in never:
    time.sleep(after)
    save({"type": "oauth"})
    print("\n\x1b[32m✓ Login successful!\x1b[0m", file=err, flush=True)
sys.stdin.read()

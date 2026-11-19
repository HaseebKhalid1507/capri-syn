"""A stand-in local model server for the shots: `GET /v1/models` as Ollama's OpenAI-compatible
endpoint answers it (`{"data": [{"id": ...}]}`), nothing else.

    python tools/fakelocal.py PORT [MODEL ...]
"""
import http.server
import json
import sys

PORT = int(sys.argv[1]) if len(sys.argv) > 1 else 47812
MODELS = sys.argv[2:] or ["qwen3:8b", "llama3.2:latest"]


class Models(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        if self.path.rstrip("/") != "/v1/models":
            self.send_error(404)
            return
        body = json.dumps({"object": "list", "data": [{"id": m, "object": "model"} for m in MODELS]}).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, fmt, *args):
        sys.stdout.write("fakelocal: " + (fmt % args) + "\n")
        sys.stdout.flush()


print(f"fake local server on 127.0.0.1:{PORT}: {', '.join(MODELS)}", flush=True)
http.server.HTTPServer(("127.0.0.1", PORT), Models).serve_forever()

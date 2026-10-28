#!/usr/bin/env python3
"""Verify the installed Codex's tier semantics against a loopback Responses peer.

No account credentials, user sessions or cloud inference are used. A temporary
CODEX_HOME has a global Fast default; the real app-server composes and sends the
HTTP requests. The peer records their service_tier and returns an empty response.
"""
import asyncio
import json
import os
from pathlib import Path
import shutil
import tempfile
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer


class ResponsesPeer(BaseHTTPRequestHandler):
    tiers = []

    def log_message(self, *_args):
        pass

    def do_POST(self):
        request = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        self.tiers.append(request.get("service_tier"))
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.end_headers()
        response = {
            "id": f"resp_{len(self.tiers)}", "object": "response", "status": "completed",
            "output": [], "usage": {"input_tokens": 1, "output_tokens": 1, "total_tokens": 2},
        }
        self.wfile.write(("data: " + json.dumps({
            "type": "response.completed", "response": response,
        }) + "\n\n").encode())
        self.wfile.flush()


class AppServer:
    def __init__(self, executable, home):
        self.executable, self.home = executable, home
        self.child = None
        self.sequence = 0

    async def start(self):
        self.child = await asyncio.create_subprocess_exec(
            self.executable, "app-server", env=dict(os.environ, CODEX_HOME=str(self.home)),
            stdin=asyncio.subprocess.PIPE, stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.DEVNULL, limit=4 * 1024 * 1024,
        )
        await self.rpc("initialize", {
            "clientInfo": {"name": "zerun-service-tier-probe", "version": "1"},
            "capabilities": {"experimentalApi": True},
        })
        self.child.stdin.write(b'{"method":"initialized"}\n')
        await self.child.stdin.drain()

    async def read(self):
        line = await asyncio.wait_for(self.child.stdout.readline(), 30)
        if not line:
            raise RuntimeError("Codex app-server closed stdout")
        return json.loads(line)

    async def rpc(self, method, params):
        self.sequence += 1
        identifier = self.sequence
        self.child.stdin.write((json.dumps({
            "id": identifier, "method": method, "params": params,
        }) + "\n").encode())
        await self.child.stdin.drain()
        while True:
            message = await self.read()
            if message.get("id") == identifier:
                if "error" in message:
                    raise RuntimeError(f"{method}: {message['error']}")
                return message["result"]

    async def turn(self, thread, tier, expected, label):
        before = len(ResponsesPeer.tiers)
        await self.rpc("turn/start", {
            "threadId": thread, "input": [{"type": "text", "text": "Reply OK without tools."}],
            **tier,
        })
        while True:
            message = await self.read()
            if message.get("method") == "turn/completed":
                assert message["params"]["turn"]["status"] == "completed", message
                break
        actual = ResponsesPeer.tiers[before:]
        assert actual and all(value == expected for value in actual), (label, actual, expected)
        print(f"PASS {label}: HTTP service_tier={expected!r}", flush=True)

    async def close(self):
        if self.child is not None and self.child.returncode is None:
            self.child.terminate()
            try:
                await asyncio.wait_for(self.child.wait(), 5)
            except asyncio.TimeoutExpired:
                self.child.kill()
                await self.child.wait()

    async def restart(self):
        await self.close()
        await self.start()


async def probe(executable, home, port):
    (home / "config.toml").write_text(f'''model = "gpt-6-astra"
model_provider = "loopback_probe"
service_tier = "fast"
[model_providers.loopback_probe]
name = "Loopback probe"
base_url = "http://127.0.0.1:{port}/v1"
wire_api = "responses"
requires_openai_auth = false
''')
    server = AppServer(executable, home)
    cases = [("omitted", {}, "priority"), ("null", {"serviceTier": None}, None),
             ("default", {"serviceTier": "default"}, None),
             ("fast", {"serviceTier": "fast"}, "priority"),
             ("priority", {"serviceTier": "priority"}, "priority")]
    try:
        await server.start()
        for label, tier, expected in cases:
            result = await server.rpc("thread/start", {
                "cwd": str(home), "approvalPolicy": "never", "sandbox": "read-only", **tier,
            })
            thread = result["thread"]["id"]
            await server.turn(thread, {}, expected, f"thread/start {label}, global=fast")
        for label, tier, expected in cases:
            await server.turn(thread, {"serviceTier": "fast"}, "priority", "enable Fast")
            await server.turn(thread, tier, expected, f"turn/start {label}, native=priority")
        for label, tier, expected in cases:
            await server.turn(thread, {"serviceTier": "fast"}, "priority", "persist Fast")
            await server.restart()
            await server.rpc("thread/resume", {"threadId": thread, **tier})
            await server.turn(thread, {}, expected, f"cold resume {label}, native=priority, global=fast")
        # Already-loaded resume can ignore overrides. Zerun must send off again
        # on turn/start rather than relying on thread setup alone.
        await server.rpc("thread/resume", {"threadId": thread, "serviceTier": "default"})
        await server.turn(thread, {}, "priority", "loaded resume retains native priority")
        await server.turn(thread, {"serviceTier": "default"}, None, "explicit off after loaded resume")
        await server.turn(thread, {}, None, "ordinary follow-up remains Standard")
        await server.restart()
        await server.rpc("thread/resume", {"threadId": thread, "serviceTier": "default"})
        await server.turn(thread, {"serviceTier": "default"}, None, "explicit off after restart, global=fast")
    finally:
        await server.close()


def main():
    executable = os.environ.get("CODEX_EXECUTABLE") or shutil.which("codex")
    if not executable:
        raise SystemExit("Codex executable not found")
    peer = ThreadingHTTPServer(("127.0.0.1", 0), ResponsesPeer)
    worker = threading.Thread(target=peer.serve_forever, daemon=True)
    worker.start()
    try:
        with tempfile.TemporaryDirectory(prefix="zerun-service-tier-") as directory:
            asyncio.run(asyncio.wait_for(probe(executable, Path(directory), peer.server_port), 180))
    finally:
        peer.shutdown()
        peer.server_close()
        worker.join()


if __name__ == "__main__":
    main()

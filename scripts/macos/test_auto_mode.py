#!/usr/bin/env python3
"""Exercise the actual launcher exec and identity protocol, without OAuth/inference.

The child is an environment recorder, not Claude Code. Real account tests are separate.
"""
import hashlib
import hmac
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import secrets
import subprocess
import sys
import tempfile
import threading
import urllib.parse
import uuid

launcher = Path(sys.argv[1] if len(sys.argv) > 1 else "target/release/claude-gpt").resolve()
passed = []
parent_auto_mode = os.environ.get("CLAUDE_CODE_AUTO_MODE_SERVER")
with tempfile.TemporaryDirectory(prefix="bridge-auto-mode-") as directory:
    home = Path(directory)
    root = home / "Library/Application Support/Claude Bridge"
    runtime = root / "runtime"
    runtime.mkdir(parents=True, mode=0o700)
    project = home / "project"
    project.mkdir()
    child = home / "native-claude-recorder"
    child.write_text("#!" + sys.executable + "\n" + '''import json, os, pathlib, sys
args = sys.argv[1:]
assert args[0] == '--settings'
settings = json.loads(pathlib.Path(args[1]).read_text())
print(json.dumps({'auto_server':os.environ.get('CLAUDE_CODE_AUTO_MODE_SERVER'),
    'session_auto_server':settings['env'].get('CLAUDE_CODE_AUTO_MODE_SERVER'),
    'base_url':os.environ.get('ANTHROPIC_BASE_URL'), 'args':args[2:],
    'permissions':settings.get('permissions'), 'cwd':os.getcwd()}))
''')
    child.chmod(0o700)
    settings = {"model":"fixture-model", "tested_model":"fixture-model", "claude_path":str(child), "installation_id":str(uuid.uuid4())}
    (root / "settings.json").write_text(json.dumps(settings))
    key = secrets.token_hex(32)
    (runtime / "local.key").write_text(key)
    (runtime / "local.key").chmod(0o600)
    native_settings = project / "original-settings.json"
    original = json.dumps({"env":{"CLAUDE_CODE_AUTO_MODE_SERVER":"1"},"permissions":{"defaultMode":"default"}})
    native_settings.write_text(original)
    (home / ".zshrc").write_text("# existing user shell configuration\n")
    before = {path: path.read_bytes() for path in [child,native_settings,home/".zshrc",root/"settings.json"]}
    requests = []

    class Identity(BaseHTTPRequestHandler):
        def do_GET(self):
            assert self.path.startswith("/bridge/identity?")
            requests.append(dict(self.headers))
            challenge = urllib.parse.parse_qs(urllib.parse.urlparse(self.path).query)["challenge"][0]
            connection = self.server.connection
            payload = b"org.claudebridge.desktop" + challenge.encode() + json.dumps(connection,separators=(",",":")).encode()
            proof = hmac.new(key.encode(), payload, hashlib.sha256).hexdigest()
            if self.server.invalid_proof:
                proof = "invalid-proof"
            body = json.dumps({"product":"org.claudebridge.desktop","connection":connection,"proof":proof}).encode()
            self.send_response(200)
            self.send_header("Content-Type","application/json")
            self.send_header("Content-Length",str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def log_message(self, *_args):
            pass

    servers = []
    try:
        for _ in range(2):
            server = ThreadingHTTPServer(("127.0.0.1",0), Identity)
            server.connection = {"base_url":f"http://127.0.0.1:{server.server_port}","installation_id":settings["installation_id"],"model":"fixture-model","pid":os.getpid(),"version":"0.2.0"}
            server.invalid_proof = False
            threading.Thread(target=server.serve_forever, daemon=True).start()
            servers.append(server)

        def run(server, arguments, succeeds=True):
            (runtime / "service.json").write_text(json.dumps(server.connection))
            env = os.environ | {"HOME":str(home),"CLAUDE_CODE_AUTO_MODE_SERVER":"1"}
            result = subprocess.run([str(launcher),*arguments,"--settings",str(native_settings)],cwd=project,env=env,capture_output=True,timeout=15)
            if not succeeds:
                assert result.returncode != 0, "Unverified identity was accepted"
                return
            assert result.returncode == 0, "Launcher did not exec the recorder"
            actual = json.loads(result.stdout)
            assert actual["auto_server"] == actual["session_auto_server"] == "0"
            assert actual["args"] == arguments and actual["permissions"] == {"defaultMode":"default"}
            assert actual["base_url"] == server.connection["base_url"]
            assert Path(actual["cwd"]).resolve() == project.resolve()

        for arguments in [[],["--resume","fixture-session"],["--continue"]]:
            run(servers[0],arguments)
            passed.append("child env and permissions: " + (" ".join(arguments) or "normal"))
        assert servers[0].server_port != servers[1].server_port
        run(servers[1],["--continue"])
        passed.append("new authenticated dynamic port accepted")
        servers[1].invalid_proof = True
        run(servers[1],[],succeeds=False)
        assert requests and all(not any(name.lower() in {"authorization","x-api-key"} for name in headers) for headers in requests)
        passed.append("invalid proof rejected; identity probe sends no credentials")
        assert all(path.read_bytes() == data for path,data in before.items())
        assert os.environ.get("CLAUDE_CODE_AUTO_MODE_SERVER") == parent_auto_mode
        passed.append("native executable, user files, and parent environment unchanged")
    finally:
        for server in servers:
            server.shutdown()
            server.server_close()
print(json.dumps({"passed":len(passed),"checks":passed,"real_claude":False,"real_upstream":False}))

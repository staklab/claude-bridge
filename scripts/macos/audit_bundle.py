#!/usr/bin/env python3
"""No credential values are printed, even on a failed audit."""
import json
from pathlib import Path
import sys

app = Path(sys.argv[1] if len(sys.argv) > 1 else "dist/Claude Bridge.app")
reference = Path.home() / ".config/claude-code-proxy/siwc/state.json"
forbidden_names = {"state.json", "auth.json", "settings.json", "local.key", ".env", "state.lock", "service.json"}
secrets = []


def collect(value):
    if isinstance(value, dict):
        for key, item in value.items():
            if key in {"access_token", "refresh_token", "id_token", "client_id", "ext_agent_host_id"} and isinstance(item, str) and len(item) > 12:
                secrets.append(item.encode())
            elif isinstance(item, (dict, list)):
                collect(item)
    elif isinstance(value, list):
        for item in value:
            collect(item)


if reference.exists():
    collect(json.loads(reference.read_bytes()))
files = [p for p in app.rglob("*") if p.is_file()]
bad_names = sum(p.name in forbidden_names for p in files)
secret_matches = sum(any(secret in p.read_bytes() for secret in secrets) for p in files)
print(json.dumps({"files_scanned": len(files), "forbidden_auth_files": bad_names, "credential_matches": secret_matches, "reference_available": bool(secrets)}))
if bad_names or secret_matches:
    raise SystemExit("Bundle credential audit failed. Values intentionally withheld.")

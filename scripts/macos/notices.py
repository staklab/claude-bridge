#!/usr/bin/env python3
"""Build-time dependency notices. This script is never needed on an end user's Mac."""
import json
from pathlib import Path
import subprocess
import sys

metadata = json.loads(subprocess.check_output(["cargo", "metadata", "--locked", "--format-version", "1", "--filter-platform", "aarch64-apple-darwin"]))
nodes = {node["id"]: node for node in metadata["resolve"]["nodes"]}
included = set()


def visit(identifier):
    if identifier in included:
        return
    included.add(identifier)
    for dependency in nodes[identifier]["deps"]:
        if any(kind["kind"] != "dev" for kind in dependency["dep_kinds"]):
            visit(dependency["pkg"])


visit(metadata["resolve"]["root"])
parts = ["Claude Bridge third-party notices\n\nThis distribution includes Rust dependencies and their transitive dependencies.\n"]
missing = []
for package in sorted(metadata["packages"], key=lambda p: (p["name"], p["version"])):
    if package["source"] is None or package["id"] not in included:
        continue
    root = Path(package["manifest_path"]).parent
    files = sorted({p for pattern in ["LICENSE*", "LICENCE*", "COPYING*", "NOTICE*", "license*", "licence*"] for p in root.rglob(pattern) if p.is_file()})
    if package.get("license_file"):
        license_file = root / package["license_file"]
        if license_file.is_file(): files.append(license_file)
    if not files:
        fallback = Path(__file__).resolve().parents[2] / "macos/third-party-licenses" / (package["name"] + ".txt")
        if fallback.is_file(): files.append(fallback)
    parts.append("\n" + "=" * 72 + f"\n{package['name']} {package['version']}\nLicense: {package.get('license', '')}\nSource: {package.get('repository') or package['source']}\n")
    if not files:
        missing.append(package["name"])
    for file in files:
        parts.append("\n" + file.name + "\n\n" + file.read_text(errors="replace"))
Path(sys.argv[1]).write_text("\n".join(parts))
print(json.dumps({"dependency_packages": len(included) - 1, "missing_notices": missing}))
if missing:
    raise SystemExit("A dependency license notice is missing; packaging stopped.")

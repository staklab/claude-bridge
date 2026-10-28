#!/usr/bin/env python3
"""Packaged-runtime acceptance in isolated homes; requires Python only on the test host.

This is not a fresh macOS VM or reboot test. Never starts a launchd job or uses OAuth.
"""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

APP = Path(sys.argv[1] if len(sys.argv) > 1 else "dist/Claude Bridge.app").resolve()
BIN = APP / "Contents/Resources/bin/claude-gpt"
SYSTEM_PATH = "/usr/bin:/bin:/usr/sbin:/sbin"
passed = []
job = f"gui/{os.getuid()}/org.claudebridge.agent"
initial_job = subprocess.run(["/bin/launchctl", "print", job], capture_output=True)


def check(condition, name):
    if not condition:
        raise AssertionError(name)
    passed.append(name)
    print("PASS", name)


with tempfile.TemporaryDirectory(prefix="claude-bridge-acceptance-") as temporary:
    base = Path(temporary)
    home = base / "clean home"
    home.mkdir()
    env = {"HOME": str(home), "PATH": SYSTEM_PATH, "TMPDIR": temporary, "LANG": "en_US.UTF-8"}
    support = home / "Library/Application Support/Claude Bridge"

    def run(*arguments, ok=True, extra=None):
        result = subprocess.run([str(BIN), "__bridge", *arguments], env=env | (extra or {}), capture_output=True, timeout=45)
        if ok and result.returncode:
            raise AssertionError(result.stderr.decode(errors="replace"))
        if not ok:
            check(result.returncode != 0, "rejected " + arguments[0])
            return result
        return json.loads(result.stdout)

    manifest = json.loads((APP / "Contents/Resources/manifest.json").read_text())
    check(all(hashlib.sha256((APP / "Contents/Resources/bin" / name).read_bytes()).hexdigest() == value for name, value in manifest["binaries"].items()), "packaged binary integrity")
    first = run("status")
    check(not first["installed"] and not first["account"]["signed_in"], "fresh home starts without credentials")
    check(not (home / ".zshrc").exists(), "no preexisting shell config required")
    run("install", str(APP), ok=False)
    check(not (home / ".local/bin/claude-gpt").exists(), "missing consent makes no launcher change")
    run("install", str(APP), "--consent")
    check((home / ".local/bin/claude-gpt").is_symlink(), "stable user command installed")
    check(run("verify-shell")["verified"], "new login zsh resolves command without development PATH")
    result = subprocess.run(["/bin/zsh", "-ic", "command -v claude-gpt"], env=env, capture_output=True, timeout=10)
    check(str(home / ".local/bin/claude-gpt") in result.stdout.decode(), "non-login interactive zsh resolves command")
    check(not any(shutil.which(name, path=SYSTEM_PATH) for name in ["cargo", "rustc", "node", "brew"]), "runtime test PATH excludes Rust Node and Homebrew")
    settings = json.loads((support / "settings.json").read_text())
    check(settings["auth_directory"] is None and settings["model"] is None, "fresh install has no imported authorization or hardcoded model")
    for file in [home / ".zprofile", home / ".zshrc"]:
        check(file.read_text().count("# >>> Claude Bridge PATH >>>") == 1, file.name + " has one managed block")
    run("install", str(APP), "--consent")
    check((home / ".zshrc").read_text().count("# >>> Claude Bridge PATH >>>") == 1, "repair does not duplicate PATH")
    check(json.loads((support / "settings.json").read_text())["installation_id"] == settings["installation_id"], "repair preserves installation identity")
    sessions = home / ".claude/projects/retained"
    sessions.mkdir(parents=True)
    history = sessions / "session.jsonl"
    history.write_text("retained historical session\n")
    auth = support / "config/siwc"
    auth.mkdir(parents=True, exist_ok=True)
    state = auth / "state.json"
    state.write_text('{"test_sentinel":"NOT_A_REAL_CREDENTIAL"}')
    run("uninstall", "--consent")
    check(not (home / ".local/bin/claude-gpt").exists(), "uninstall removes owned launcher")
    check("Claude Bridge PATH" not in (home / ".zshrc").read_text(), "uninstall removes only managed shell block")
    check(history.read_text() == "retained historical session\n" and "NOT_A_REAL_CREDENTIAL" in state.read_text(), "uninstall retains sessions and account data")
    run("install", str(APP), "--consent")
    check("NOT_A_REAL_CREDENTIAL" in state.read_text(), "reinstall retains existing account data")
    run("uninstall", "--consent")
    # A native Claude installation is reused and never replaced or removed.
    native = home / ".local/bin/claude"
    native.write_text("#!/bin/sh\nprintf 'native Claude sentinel\\n'\n")
    native.chmod(0o700)
    native_hash = hashlib.sha256(native.read_bytes()).hexdigest()
    (home / ".zshrc").write_text("# user setting\nexport USER_SENTINEL=keep\n")
    run("install", str(APP), "--consent")
    check(run("status")["claude_path"] == str(native), "existing Claude executable reused")
    check(list(home.glob(".zshrc.claude-bridge-backup-*")), "shell configuration backed up before edit")
    run("uninstall", "--consent")
    check(hashlib.sha256(native.read_bytes()).hexdigest() == native_hash and history.exists(), "native Claude and history survive complete lifecycle")
    check("export USER_SENTINEL=keep" in (home / ".zshrc").read_text(), "uninstall preserves unrelated shell settings")
    # Collision must not replace a user's command.
    foreign = home / ".local/bin/claude-gpt"
    foreign.write_text("do not replace")
    run("install", str(APP), "--consent", ok=False)
    check(foreign.read_text() == "do not replace", "foreign command preserved")
    foreign.unlink()
    # Symlinked shell config must not change its target.
    config = home / ".zshrc"
    config.unlink()
    outside = base / "outside-config"
    outside.write_text("do not edit")
    config.symlink_to(outside)
    run("install", str(APP), "--consent", ok=False)
    check(outside.read_text() == "do not edit", "symlink target preserved")
    config.unlink()
    # ZDOTDIR set by .zshenv must be detected even when GUI did not inherit it.
    custom = home / "custom-zsh"
    custom.mkdir()
    (home / ".zshenv").write_text('export ZDOTDIR="$HOME/custom-zsh"\n')
    run("install", str(APP), "--consent")
    check((custom / ".zshrc").exists() and run("verify-shell")["verified"], "GUI detects ZDOTDIR from zshenv")
    run("uninstall", "--consent")
    # Tampered install payload rejected before writes.
    broken = base / "Tampered.app"
    shutil.copytree(APP, broken, symlinks=True)
    with (broken / "Contents/Resources/bin/claude-code-proxy").open("ab") as handle:
        handle.write(b"tampered")
    run("install", str(broken), "--consent", ok=False)
    check(not foreign.exists(), "tampered package cannot create command")

if initial_job.returncode == 0:
    final_job = subprocess.run(["/bin/launchctl", "print", job], capture_output=True)
    original_program = next((line.strip() for line in initial_job.stdout.splitlines() if line.strip().startswith(b"program = ")), None)
    check(final_job.returncode == 0 and original_program in final_job.stdout, "isolated HOME cannot unload another installation's launchd job")
print(json.dumps({"passed": len(passed), "fresh_os_test": False, "reboot_test": False, "oauth_test": False}))

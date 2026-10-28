#!/usr/bin/env python3
"""Opt-in live acceptance. Requires the user's completed GUI setup and uses plan quota.

Run only after closing the App to test the CLI's independence. Creates disposable
synthetic projects and an isolated native Claude configuration, never user projects.
"""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import uuid

launcher = Path.home() / ".local/bin/claude-gpt"
assert launcher.is_file(), "Complete the GUI setup first"
with tempfile.TemporaryDirectory(prefix="claude-bridge-live-") as directory:
    project = Path(directory) / "project"
    project.mkdir()
    config = Path(directory) / "native-config"
    config.mkdir()
    env = os.environ | {"CLAUDE_CONFIG_DIR": str(config), "PATH": str(Path.home() / ".local/bin") + ":/usr/bin:/bin:/usr/sbin:/sbin"}
    # API connection values from the calling shell must not change the bridge route.
    env["ANTHROPIC_BASE_URL"] = "https://must-not-be-used.invalid"
    env["CLAUDE_CODE_USE_BEDROCK"] = "1"
    session = str(uuid.uuid4())

    def run(prompt, more):
        result = subprocess.run([str(launcher), "-p", prompt, "--output-format", "stream-json", "--verbose", "--strict-mcp-config", "--mcp-config", '{"mcpServers":{}}', *more], cwd=project, env=env, capture_output=True, timeout=180)
        assert result.returncode == 0, "Native Claude GPT exited with error (output intentionally not echoed)"
        events = [json.loads(line) for line in result.stdout.decode().splitlines() if line.startswith("{")]
        final = next((event for event in reversed(events) if event.get("type") == "result"), None)
        assert final and not final.get("is_error"), "Native request did not complete"
        return events, final

    _, text = run("Reply exactly: CLAUDE BRIDGE OK", ["--session-id", session, "--max-turns", "2"])
    assert "CLAUDE BRIDGE OK" in text["result"]
    print("PASS native text conversation through installed claude-gpt", flush=True)
    _, resumed = run("What exact phrase did you just reply? Repeat it only.", ["--resume", session, "--max-turns", "2"])
    assert "CLAUDE BRIDGE OK" in resumed["result"]
    print("PASS --resume finds the current project's native session", flush=True)
    _, continued = run("Reply exactly: CONTINUE OK", ["--continue", "--max-turns", "2"])
    assert "CONTINUE OK" in continued["result"]
    print("PASS --continue preserves native behavior", flush=True)
    nonce = "READ-" + uuid.uuid4().hex
    (project / "source.txt").write_text(nonce + "\n")
    prompt = "In this disposable test project: first use Read to read source.txt; then use Write to write its exact contents to target.txt; then use Bash to run: pwd > bash-cwd.txt . Use each named tool at least once. End by replying TOOLS OK. Do not access any other directory."
    events, tools = run(prompt, ["--allowedTools", "Read,Write,Bash", "--max-turns", "8"])
    calls = {block.get("name") for event in events if event.get("type") == "assistant" for block in event.get("message", {}).get("content", []) if block.get("type") == "tool_use"}
    assert {"Read", "Write", "Bash"} <= calls, "Missing a real native tool call"
    assert (project / "target.txt").read_text().strip() == nonce
    assert Path((project / "bash-cwd.txt").read_text().strip()).resolve() == project.resolve()
    print("PASS real Read Write Bash calls and working directory", flush=True)
    print(json.dumps({"text": True, "resume": True, "continue": True, "native_tools": sorted(calls), "app_may_be_closed": True, "fresh_oauth": False, "real_reboot": False}))

#!/bin/bash
set -euo pipefail
TASK_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
TASK_TEMP="$(mktemp -d -t bridge-terminal-tests)"
trap 'rm -rf "$TASK_TEMP"' EXIT
cd "$TASK_ROOT"
/usr/bin/swiftc -swift-version 5 -parse-as-library macos/ClaudeBridge/TerminalLauncher.swift scripts/macos/test_terminal.swift -o "$TASK_TEMP/test-terminal"
"$TASK_TEMP/test-terminal"

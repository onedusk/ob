#!/usr/bin/env python3
"""
Example logging hook that tracks bash commands.
This demonstrates how to add hooks in different categories.
"""

import json
import sys
import os
from datetime import datetime
from pathlib import Path

# Log file location
LOG_DIR = Path.home() / ".claude" / "logs"
LOG_FILE = LOG_DIR / "bash_commands.jsonl"


def ensure_log_dir():
    """Ensure log directory exists."""
    LOG_DIR.mkdir(parents=True, exist_ok=True)


def log_command(input_data):
    """Log bash command to file."""
    tool_name = input_data.get("tool_name", "")

    if tool_name != "Bash":
        return

    tool_input = input_data.get("tool_input", {})
    command = tool_input.get("command", "")
    description = tool_input.get("description", "")

    log_entry = {
        "timestamp": datetime.now().isoformat(),
        "session_id": input_data.get("session_id", ""),
        "cwd": input_data.get("cwd", ""),
        "command": command,
        "description": description
    }

    ensure_log_dir()

    # Append to log file (JSONL format - one JSON object per line)
    with open(LOG_FILE, "a") as f:
        json.dump(log_entry, f)
        f.write("\n")


def main():
    """Main entry point for the hook."""
    try:
        # Read input from stdin
        input_data = json.load(sys.stdin)
    except json.JSONDecodeError:
        sys.exit(1)

    # Log the command
    try:
        log_command(input_data)
    except Exception:
        # Don't fail the hook if logging fails
        pass

    # Always exit successfully to not block operations
    sys.exit(0)


if __name__ == "__main__":
    main()
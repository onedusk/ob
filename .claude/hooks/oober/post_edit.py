#!/usr/bin/env python3
"""
Claude Code PostToolUse hook for oober integration.
Tracks edit patterns and suggests oober when appropriate.
"""

import json
import sys
import os
from pathlib import Path
from datetime import datetime, timedelta

# Track recent edits to detect patterns
EDIT_HISTORY_FILE = "/tmp/claude_edit_history.json"
PATTERN_THRESHOLD = 3  # Suggest oober after 3 similar edits
TIME_WINDOW = 300  # Consider edits within 5 minutes


def load_edit_history():
    """Load recent edit history from temporary file."""
    if not os.path.exists(EDIT_HISTORY_FILE):
        return []

    try:
        with open(EDIT_HISTORY_FILE, 'r') as f:
            history = json.load(f)

        # Filter out old entries (older than TIME_WINDOW seconds)
        cutoff = datetime.now().timestamp() - TIME_WINDOW
        return [e for e in history if e.get("timestamp", 0) > cutoff]
    except:
        return []


def save_edit_history(history):
    """Save edit history to temporary file."""
    try:
        with open(EDIT_HISTORY_FILE, 'w') as f:
            json.dump(history, f)
    except:
        pass


def analyze_pattern(history):
    """Analyze edit history for patterns that could benefit from oober."""
    if len(history) < PATTERN_THRESHOLD:
        return None

    # Check for similar replacements
    replacements = {}
    for edit in history:
        key = f"{edit.get('old', '')}→{edit.get('new', '')}"
        replacements[key] = replacements.get(key, 0) + 1

    # Find most common replacement
    for pattern, count in replacements.items():
        if count >= PATTERN_THRESHOLD:
            old, new = pattern.split('→')
            return {
                "pattern": old,
                "replacement": new,
                "count": count,
                "files": list(set([e.get("file") for e in history if f"{e.get('old')}→{e.get('new')}" == pattern]))
            }

    return None


def main():
    """Main entry point for the hook."""
    try:
        # Read input from stdin
        input_data = json.load(sys.stdin)
    except json.JSONDecodeError:
        sys.exit(1)

    # Only process Edit/MultiEdit tools
    tool_name = input_data.get("tool_name", "")
    if tool_name not in ["Edit", "MultiEdit"]:
        sys.exit(0)

    # Extract edit information
    tool_input = input_data.get("tool_input", {})
    tool_response = input_data.get("tool_response", {})

    # Skip if edit failed
    if not tool_response.get("success", False):
        sys.exit(0)

    # Load history
    history = load_edit_history()

    # Add current edit to history
    if tool_name == "Edit":
        history.append({
            "timestamp": datetime.now().timestamp(),
            "file": tool_input.get("file_path", ""),
            "old": tool_input.get("old_string", "")[:100],  # Truncate for storage
            "new": tool_input.get("new_string", "")[:100],
        })
    elif tool_name == "MultiEdit":
        for edit in tool_input.get("edits", []):
            history.append({
                "timestamp": datetime.now().timestamp(),
                "file": tool_input.get("file_path", ""),
                "old": edit.get("old_string", "")[:100],
                "new": edit.get("new_string", "")[:100],
            })

    # Save updated history
    save_edit_history(history)

    # Check for patterns
    pattern = analyze_pattern(history)

    if pattern:
        # Suggest using oober
        project_dir = os.environ.get("CLAUDE_PROJECT_DIR", ".")

        output = {
            "hookSpecificOutput": {
                "hookEventName": "PostToolUse",
                "additionalContext": (
                    f"\n💡 Pattern Detected: You've made {pattern['count']} similar edits "
                    f"(replacing '{pattern['pattern'][:30]}...' with '{pattern['replacement'][:30]}...').\n\n"
                    f"Consider using oober for bulk operations:\n"
                    f"```bash\n"
                    f"ob replace -d {project_dir} -p '{pattern['pattern']}' -r '{pattern['replacement']}'\n"
                    f"```\n"
                    f"This would be more efficient and create automatic backups."
                )
            }
        }

        print(json.dumps(output))

    sys.exit(0)


if __name__ == "__main__":
    main()
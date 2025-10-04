#!/usr/bin/env python3
"""
Claude Code PreToolUse hook for oober integration.
Intercepts Edit/MultiEdit operations and determines if oober would be more efficient.
"""

import json
import sys
import re
import os
import subprocess
from pathlib import Path
from typing import Dict, List, Tuple, Optional

# Configuration for when to use oober
BULK_EDIT_THRESHOLD = 5  # Use oober if editing > 5 files
MAX_FILES_AUTO_APPROVE = 10  # Auto-approve oober for <= 10 files

# Patterns that suggest oober should be used
OOBER_PATTERNS = [
    r"s/([^/]+)/([^/]+)/g?",  # Simple substitution patterns
    r"TODO.*DONE",  # TODO replacements
    r"console\.log",  # Debug statement removal
    r"import.*from",  # Import updates
    r"\brename\b",  # Renaming operations
    r"replace.*all",  # Bulk replacements
]


def analyze_edits(tool_input: Dict) -> Tuple[bool, str, Optional[Dict]]:
    """
    Analyze Edit/MultiEdit operations to determine if oober should be used.

    Returns:
        Tuple of (should_use_oober, reason, oober_config)
    """
    tool_name = tool_input.get("tool_name", "")

    # Only intercept Edit and MultiEdit tools
    if tool_name not in ["Edit", "MultiEdit"]:
        return False, "Not an edit operation", None

    # For MultiEdit, check if it's a pattern-based edit
    if tool_name == "MultiEdit":
        edits = tool_input.get("tool_input", {}).get("edits", [])
        if len(edits) > BULK_EDIT_THRESHOLD:
            # Analyze if edits follow a pattern
            patterns = analyze_edit_patterns(edits)
            if patterns:
                return True, f"Pattern-based MultiEdit with {len(edits)} changes", patterns

    # For single Edit, check if it looks like a pattern replacement
    if tool_name == "Edit":
        old_string = tool_input.get("tool_input", {}).get("old_string", "")
        new_string = tool_input.get("tool_input", {}).get("new_string", "")

        # Check if this looks like a pattern-based replacement
        for pattern in OOBER_PATTERNS:
            if re.search(pattern, old_string, re.IGNORECASE):
                return True, f"Pattern-based edit matching: {pattern}", {
                    "pattern": old_string,
                    "replacement": new_string
                }

    return False, "Standard edit is appropriate", None


def analyze_edit_patterns(edits: List[Dict]) -> Optional[Dict]:
    """
    Analyze multiple edits to find common patterns.

    Returns:
        Dict with pattern and replacement if pattern found, None otherwise
    """
    if not edits:
        return None

    # Check if all edits are similar (e.g., all replacing TODO with DONE)
    old_strings = [e.get("old_string", "") for e in edits]
    new_strings = [e.get("new_string", "") for e in edits]

    # Simple pattern detection: all same replacement
    if len(set(old_strings)) == 1 and len(set(new_strings)) == 1:
        return {
            "pattern": old_strings[0],
            "replacement": new_strings[0]
        }

    # More complex pattern detection could go here
    # For now, return None if no simple pattern found
    return None


def generate_oober_command(config: Dict, file_path: Optional[str] = None) -> str:
    """Generate the oober command based on the configuration."""
    cmd = ["ob", "replace"]

    # Determine directory
    if file_path:
        dir_path = str(Path(file_path).parent)
    else:
        dir_path = os.environ.get("CLAUDE_PROJECT_DIR", ".")

    cmd.extend(["-d", dir_path])

    # Add pattern and replacement
    if config.get("pattern"):
        cmd.extend(["-p", f"'{config['pattern']}'"])

    if config.get("replacement") is not None:
        cmd.extend(["-r", f"'{config['replacement']}'"])

    # Always dry-run first
    cmd.append("--dry-run")

    return " ".join(cmd)


def main():
    """Main entry point for the hook."""
    try:
        # Read input from stdin
        input_data = json.load(sys.stdin)
    except json.JSONDecodeError as e:
        print(f"Error parsing input: {e}", file=sys.stderr)
        sys.exit(1)

    # Extract relevant information
    tool_name = input_data.get("tool_name", "")
    tool_input = input_data.get("tool_input", {})

    # Check if this is an edit operation we should intercept
    input_data["tool_name"] = tool_name
    input_data["tool_input"] = tool_input
    should_use, reason, config = analyze_edits(input_data)

    if not should_use:
        # Let the edit proceed normally
        sys.exit(0)

    # Generate oober command
    file_path = tool_input.get("file_path")
    oober_cmd = generate_oober_command(config, file_path)

    # Prepare response to suggest using oober instead
    output = {
        "hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "permissionDecision": "ask",
            "permissionDecisionReason": (
                f"🚀 Oober Optimization Available\n\n"
                f"Detected: {reason}\n\n"
                f"Instead of individual edits, you could use oober for more efficient bulk operations:\n"
                f"```bash\n{oober_cmd}\n```\n\n"
                f"This would be faster and create automatic backups.\n\n"
                f"Choose:\n"
                f"• Approve: Continue with standard Claude edit\n"
                f"• Deny: Let me use oober instead"
            )
        }
    }

    print(json.dumps(output))
    sys.exit(0)


if __name__ == "__main__":
    main()
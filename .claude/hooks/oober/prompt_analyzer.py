#!/usr/bin/env python3
"""
Claude Code UserPromptSubmit hook for oober integration.
Analyzes user prompts and adds context about oober when bulk operations are requested.
"""

import json
import sys
import re

# Patterns that suggest bulk operations
BULK_OPERATION_PATTERNS = [
    (r"\b(rename|replace|update|change|fix|remove|delete)\s+all\b", "bulk_all"),
    (r"\b(everywhere|throughout|across)\b.*\b(codebase|project|files)\b", "bulk_scope"),
    (r"\b(all|every)\s+(instance|occurrence|file|match)", "bulk_target"),
    (r"(refactor|clean\s*up|standardize)", "refactor"),
    (r"remove\s+(all\s+)?(console\.log|debug|todo|fixme)", "cleanup"),
    (r"update\s+(all\s+)?imports?\s+from", "import_update"),
    (r"rename\s+\w+\s+to\s+\w+", "rename"),
]


def analyze_prompt(prompt: str) -> tuple[bool, str]:
    """
    Analyze the prompt for bulk operation indicators.

    Returns:
        Tuple of (is_bulk_operation, operation_type)
    """
    prompt_lower = prompt.lower()

    for pattern, op_type in BULK_OPERATION_PATTERNS:
        if re.search(pattern, prompt_lower):
            return True, op_type

    return False, ""


def generate_oober_context(prompt: str, operation_type: str) -> str:
    """Generate contextual information about using oober."""
    context = "\n---\n🔧 Oober Context: "

    if operation_type == "rename":
        # Try to extract rename pattern
        match = re.search(r"rename\s+(\w+)\s+to\s+(\w+)", prompt, re.IGNORECASE)
        if match:
            old, new = match.groups()
            context += (
                f"For renaming '{old}' to '{new}', you can use:\n"
                f"```bash\n"
                f"ob replace -d . -p '\\b{old}\\b' -r '{new}'\n"
                f"```\n"
            )
    elif operation_type == "cleanup":
        context += (
            "For removing debug code, you can use:\n"
            "```bash\n"
            "ob replace -d . --preset CleanDebug\n"
            "```\n"
        )
    elif operation_type in ["bulk_all", "bulk_scope", "bulk_target"]:
        context += (
            "For bulk replacements across the codebase, oober provides:\n"
            "- Parallel processing for speed\n"
            "- Automatic backup creation\n"
            "- Dry-run previews\n"
            "- Pattern-based replacements\n\n"
            "Example: `ob replace -d . -p 'pattern' -r 'replacement'`\n"
        )
    else:
        context += (
            "Consider using oober (`ob`) for efficient bulk operations.\n"
            "It provides automatic backups and parallel processing.\n"
        )

    context += (
        "\nIf appropriate, I'll suggest using oober commands for bulk operations.\n"
        "---\n"
    )

    return context


def main():
    """Main entry point for the hook."""
    try:
        # Read input from stdin
        input_data = json.load(sys.stdin)
    except json.JSONDecodeError:
        sys.exit(1)

    # Extract the user's prompt
    prompt = input_data.get("prompt", "")

    # Analyze the prompt
    is_bulk, op_type = analyze_prompt(prompt)

    if is_bulk:
        # Add context about oober
        context = generate_oober_context(prompt, op_type)

        output = {
            "hookSpecificOutput": {
                "hookEventName": "UserPromptSubmit",
                "additionalContext": context
            }
        }

        print(json.dumps(output))

    sys.exit(0)


if __name__ == "__main__":
    main()
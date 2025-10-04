#!/usr/bin/env python3
"""
Documentation Context Preservation Hook for Claude Code
Ensures critical documentation references are preserved during context compaction
"""
import json
import sys
import os
from pathlib import Path

def get_critical_documentation():
    """Identify critical documentation that should be preserved."""

    critical_docs = {
        "Core Documentation": [
            "docs/claude-code/doc-index.md",
            "docs/claude-code/hooks.md",
            "docs/claude-code/sub-agents.md",
            "docs/claude-code/memory.md"
        ],
        "Project Context": [
            ".claude/context/implementation-patterns.md",
            ".claude/context/current-focus.md",
            "CLAUDE.md"
        ],
        "Active Configurations": [
            ".claude/settings.json",
            ".claude/settings.local.json"
        ]
    }

    return critical_docs

def extract_key_patterns(file_path):
    """Extract key patterns from a file."""
    try:
        full_path = Path(file_path)
        if full_path.exists():
            with open(full_path, 'r', encoding='utf-8') as f:
                content = f.read()

            # Extract section headers (lines starting with ##)
            headers = []
            for line in content.split('\n'):
                if line.startswith('## ') and not line.startswith('## '):
                    headers.append(line)

            return headers[:5]  # Return top 5 headers
    except Exception:
        pass
    return []

def generate_preservation_summary():
    """Generate a summary of critical context to preserve."""

    summary_parts = [
        "=" * 80,
        "CRITICAL DOCUMENTATION CONTEXT TO PRESERVE",
        "=" * 80,
        "",
        "## Essential Documentation References",
        ""
    ]

    critical_docs = get_critical_documentation()

    for category, docs in critical_docs.items():
        summary_parts.append(f"### {category}")
        for doc in docs:
            if Path(doc).exists():
                summary_parts.append(f"- ✅ {doc}")
                # Add key sections from the doc
                headers = extract_key_patterns(doc)
                if headers:
                    for header in headers[:3]:  # Show top 3 headers
                        summary_parts.append(f"  {header}")
            else:
                summary_parts.append(f"- ⚠️  {doc} (not found)")
        summary_parts.append("")

    # Add quick command reference
    summary_parts.extend([
        "## Quick Command Reference",
        "- `/memory` - View or edit memory files",
        "- `/agents` - Manage subagents",
        "- `/hooks` - Configure automation",
        "- `/compact` - Manual context compaction",
        "",
        "## Active Development Focus",
        ""
    ])

    # Try to extract current focus
    focus_file = Path(".claude/context/current-focus.md")
    if focus_file.exists():
        try:
            with open(focus_file, 'r') as f:
                lines = f.read().split('\n')
                # Find "Current Implementation" section
                in_section = False
                for line in lines:
                    if "Current Implementation" in line:
                        in_section = True
                        summary_parts.append(line)
                    elif in_section and line.strip():
                        if line.startswith('#'):
                            break  # Stop at next section
                        summary_parts.append(line)
        except Exception:
            pass

    summary_parts.extend([
        "",
        "## Context Preservation Note",
        "This summary preserves critical documentation references during compaction.",
        "Full documentation remains available in the filesystem.",
        "",
        "=" * 80
    ])

    return '\n'.join(summary_parts)

def main():
    """Main execution."""
    try:
        # Read input from stdin
        input_data = json.load(sys.stdin)

        # Extract compaction information
        trigger = input_data.get("trigger", "unknown")
        custom_instructions = input_data.get("custom_instructions", "")

        # Log compaction trigger
        sys.stderr.write(f"Context compaction triggered ({trigger})\n")

        # Generate preservation summary
        summary = generate_preservation_summary()

        # Output the summary to be included in compaction
        print(summary)

        # Log what we preserved
        sys.stderr.write("Documentation context preserved for post-compaction reference\n")

        sys.exit(0)

    except Exception as e:
        sys.stderr.write(f"Error preserving documentation context: {e}\n")
        # Don't block compaction on error
        sys.exit(0)

if __name__ == "__main__":
    main()
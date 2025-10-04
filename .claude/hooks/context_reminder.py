#!/usr/bin/env python3
"""
Context Reminder Hook for Claude Code
Provides helpful reminders about documentation and patterns after file operations
"""
import json
import sys
import os
from pathlib import Path

def get_relevant_docs(file_path, tool_name):
    """Determine relevant documentation based on file and operation."""

    reminders = []
    file_ext = Path(file_path).suffix.lower()
    file_name = Path(file_path).name

    # Hook-related files
    if ".claude/hooks" in file_path or "hook" in file_name.lower():
        reminders.append("📚 Hook patterns: Check docs/claude-code/hooks.md for hook implementation patterns")
        reminders.append("🔍 Exit codes: 0=success, 2=block with feedback")
        reminders.append("💡 JSON output provides fine-grained control")

    # Subagent files
    if ".claude/agents" in file_path or "agent" in file_name.lower():
        reminders.append("🤖 Subagent guide: See docs/claude-code/sub-agents.md")
        reminders.append("🎯 Single responsibility principle for focused agents")
        reminders.append("🔒 Limit tool access for security")

    # Memory files
    if "CLAUDE.md" in file_name or "CLAUDE.local.md" in file_name:
        reminders.append("🧠 Memory system: Review docs/claude-code/memory.md")
        reminders.append("📦 Use @imports for modular organization")
        reminders.append("🏗️ Hierarchical loading: Enterprise → Project → User → Local")

    # Python files
    if file_ext == ".py":
        reminders.append("🐍 Python patterns in .claude/context/implementation-patterns.md")
        if "test" in file_path.lower():
            reminders.append("🧪 Follow testing patterns from documentation")

    # Rust files
    if file_ext == ".rs":
        reminders.append("🦀 Rust patterns: Prioritize reliability, speed, efficiency")
        reminders.append("⚡ Use Rayon for parallelization where appropriate")
        reminders.append("🔒 Ensure Send + Sync bounds for parallel safety")

    # Documentation files
    if file_ext in [".md", ".mdx"]:
        reminders.append("📝 Keep documentation synchronized with code")
        reminders.append("🔗 Update doc-index.md if adding new guides")

    # Configuration files
    if file_ext in [".json", ".yaml", ".yml", ".toml"]:
        reminders.append("⚙️ Validate configuration syntax")
        reminders.append("📋 Document any new configuration options")

    return reminders

def should_provide_reminder(tool_name, file_path):
    """Determine if a reminder should be provided."""

    # Always remind for these tools
    if tool_name in ["Write", "MultiEdit"]:
        return True

    # Remind for Edit on important files
    if tool_name == "Edit":
        important_patterns = [
            ".claude/", "hook", "agent", "CLAUDE",
            "pattern", "config", "test", "spec"
        ]
        return any(pattern in file_path for pattern in important_patterns)

    return False

def format_reminder_message(reminders, file_path):
    """Format reminders into a helpful message."""

    if not reminders:
        return None

    message_parts = [
        f"💭 Context reminder for {Path(file_path).name}:",
        ""
    ]

    for reminder in reminders:
        message_parts.append(f"  {reminder}")

    message_parts.extend([
        "",
        "📖 Full documentation index: docs/claude-code/doc-index.md"
    ])

    return '\n'.join(message_parts)

def main():
    """Main execution."""
    try:
        # Read input from stdin
        input_data = json.load(sys.stdin)

        # Extract relevant information
        tool_name = input_data.get("tool_name", "")
        tool_input = input_data.get("tool_input", {})
        file_path = tool_input.get("file_path", "")

        # Check if we should provide a reminder
        if not file_path or not should_provide_reminder(tool_name, file_path):
            sys.exit(0)

        # Get relevant documentation reminders
        reminders = get_relevant_docs(file_path, tool_name)

        if reminders:
            # Format the reminder message
            message = format_reminder_message(reminders, file_path)

            if message:
                # Output to stderr so it's visible but doesn't block
                sys.stderr.write(message + "\n")

        # Always exit successfully (non-blocking)
        sys.exit(0)

    except Exception as e:
        # Log error but don't block operation
        sys.stderr.write(f"Context reminder error: {e}\n")
        sys.exit(0)

if __name__ == "__main__":
    main()
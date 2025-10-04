#!/usr/bin/env python3
"""
Smart Documentation Loader for Claude Code
Dynamically loads relevant documentation based on working directory and context
"""
import json
import sys
import os
from pathlib import Path

def load_documentation(cwd):
    """Load relevant documentation based on current working directory."""

    # Documentation mapping based on project areas
    doc_mappings = {
        # Oober-specific patterns
        "utils/oober": {
            "primary": [
                "docs/claude-code/hooks.md",
                "docs/claude-code/sub-agents.md",
                "docs/claude-code/memory.md"
            ],
            "context": [
                ".claude/context/implementation-patterns.md",
                ".claude/context/current-focus.md"
            ]
        },
        # Add more project-specific mappings here
        "src": {
            "primary": [
                "docs/architecture.md",
                "docs/api-reference.md"
            ],
            "context": []
        },
        "tests": {
            "primary": [
                "docs/testing-guide.md",
                "docs/test-patterns.md"
            ],
            "context": []
        }
    }

    # Find the most specific mapping
    context_docs = []
    cwd_path = Path(cwd)

    for pattern, docs in doc_mappings.items():
        if pattern in str(cwd_path):
            context_docs = docs
            break

    # If no specific mapping, try to find general documentation
    if not context_docs:
        # Look for documentation in standard locations
        possible_docs = [
            "docs/README.md",
            "docs/index.md",
            "README.md",
            ".claude/context/implementation-patterns.md"
        ]

        context_docs = {
            "primary": [],
            "context": []
        }

        for doc in possible_docs:
            doc_path = cwd_path / doc
            if doc_path.exists():
                if "context" in doc or ".claude" in doc:
                    context_docs["context"].append(doc)
                else:
                    context_docs["primary"].append(doc)

    return context_docs

def read_file_safely(file_path, base_dir):
    """Safely read a file with error handling."""
    try:
        full_path = Path(base_dir) / file_path
        if full_path.exists():
            with open(full_path, 'r', encoding='utf-8') as f:
                return f.read()
    except Exception as e:
        return f"[Error reading {file_path}: {e}]"
    return None

def format_documentation_context(docs, base_dir):
    """Format documentation into context string."""
    context_parts = []

    # Add header
    context_parts.append("=" * 80)
    context_parts.append("DOCUMENTATION CONTEXT LOADED AT SESSION START")
    context_parts.append("=" * 80)
    context_parts.append("")

    # Load primary documentation
    if docs.get("primary"):
        context_parts.append("## Primary Documentation")
        context_parts.append("")
        for doc_path in docs["primary"]:
            content = read_file_safely(doc_path, base_dir)
            if content:
                context_parts.append(f"### {doc_path}")
                context_parts.append("")
                # Limit content to first 100 lines to avoid overwhelming context
                lines = content.split('\n')[:100]
                context_parts.append('\n'.join(lines))
                if len(content.split('\n')) > 100:
                    context_parts.append(f"\n... (truncated, see full file at {doc_path})")
                context_parts.append("")
                context_parts.append("-" * 40)
                context_parts.append("")

    # Load context documentation
    if docs.get("context"):
        context_parts.append("## Project Context")
        context_parts.append("")
        for doc_path in docs["context"]:
            content = read_file_safely(doc_path, base_dir)
            if content:
                context_parts.append(f"### {doc_path}")
                context_parts.append("")
                context_parts.append(content)
                context_parts.append("")
                context_parts.append("-" * 40)
                context_parts.append("")

    # Add quick reference
    context_parts.append("## Quick Reference")
    context_parts.append("")
    context_parts.append("- Use `/memory` to view or edit memory files")
    context_parts.append("- Use `/agents` to manage subagents")
    context_parts.append("- Use `/hooks` to configure automation")
    context_parts.append("- Documentation index: docs/claude-code/doc-index.md")
    context_parts.append("")
    context_parts.append("=" * 80)

    return '\n'.join(context_parts)

def main():
    """Main execution."""
    try:
        # Read input from stdin
        input_data = json.load(sys.stdin)

        # Extract session information
        session_id = input_data.get("session_id", "unknown")
        cwd = input_data.get("cwd", os.getcwd())
        source = input_data.get("source", "unknown")

        # Log session start
        sys.stderr.write(f"Starting session {session_id} in {cwd} (source: {source})\n")

        # Load relevant documentation
        docs = load_documentation(cwd)

        if docs and (docs.get("primary") or docs.get("context")):
            # Format documentation as context
            context = format_documentation_context(docs, cwd)

            # Return as additional context
            output = {
                "hookSpecificOutput": {
                    "hookEventName": "SessionStart",
                    "additionalContext": context
                }
            }

            print(json.dumps(output))
            sys.stderr.write(f"Loaded {len(docs.get('primary', []))} primary docs and {len(docs.get('context', []))} context docs\n")
        else:
            sys.stderr.write("No specific documentation loaded for this directory\n")

        sys.exit(0)

    except Exception as e:
        sys.stderr.write(f"Error in smart_doc_loader: {e}\n")
        sys.exit(1)

if __name__ == "__main__":
    main()
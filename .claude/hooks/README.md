# Claude Code Hooks Organization

This directory contains hooks organized by functionality to keep the codebase maintainable as we add more automation.

## Directory Structure

```
.claude/hooks/
├── oober/              # Bulk operation optimization hooks
│   ├── prompt_analyzer.py  # UserPromptSubmit: Detects bulk operations
│   ├── pre_edit.py         # PreToolUse: Intercepts edits
│   └── post_edit.py        # PostToolUse: Tracks patterns
│
├── formatting/         # Code formatting hooks (future)
│   ├── prettier.py         # Format JS/TS files
│   ├── black.py           # Format Python files
│   └── rustfmt.py         # Format Rust files
│
├── validation/         # Code validation hooks (future)
│   ├── eslint.py          # Validate JavaScript
│   ├── mypy.py            # Type check Python
│   └── security.py        # Security checks
│
├── logging/           # Activity logging hooks (future)
│   ├── command_logger.py   # Log all bash commands
│   ├── edit_tracker.py     # Track file modifications
│   └── metrics.py         # Collect usage metrics
│
└── utils/             # Shared utilities
    └── common.py          # Shared functions for hooks
```

## Adding New Hooks

1. **Choose the appropriate directory** based on functionality
2. **Create your hook script** with a descriptive name
3. **Make it executable**: `chmod +x your_hook.py`
4. **Update settings.json** to register the hook

## Hook Naming Convention

- Use descriptive names that indicate the hook's purpose
- Prefer snake_case for Python scripts
- Include the event type if not obvious (e.g., `pre_edit.py`, `post_build.sh`)

## Example: Adding a Prettier Formatting Hook

1. Create the hook:
```bash
touch .claude/hooks/formatting/prettier.py
chmod +x .claude/hooks/formatting/prettier.py
```

2. Add to settings.json:
```json
{
  "hooks": {
    "PostToolUse": [
      {
        "matcher": "Write|Edit",
        "hooks": [
          {
            "type": "command",
            "command": "$CLAUDE_PROJECT_DIR/.claude/hooks/formatting/prettier.py"
          }
        ]
      }
    ]
  }
}
```

## Current Active Hooks

### Oober Integration (Active)
- **Purpose**: Optimize bulk operations using oober
- **Events**: UserPromptSubmit, PreToolUse, PostToolUse
- **Status**: ✅ Active

### Formatting Hooks (Planned)
- **Purpose**: Auto-format code after edits
- **Events**: PostToolUse
- **Status**: 🔜 Planned

### Validation Hooks (Planned)
- **Purpose**: Validate code quality and security
- **Events**: PreToolUse, PostToolUse
- **Status**: 🔜 Planned

## Testing Hooks

Test individual hooks manually:

```bash
# Test prompt analyzer
echo '{"prompt": "rename all TODO to DONE"}' | .claude/hooks/oober/prompt_analyzer.py

# Test with specific tool input
echo '{"tool_name": "Edit", "tool_input": {"file_path": "test.js", "old_string": "TODO", "new_string": "DONE"}}' | .claude/hooks/oober/pre_edit.py
```

## Debugging

Enable debug mode to see hook execution:
```bash
claude --debug
```

Check hook registration:
```bash
# In Claude Code
/hooks
```

## Best Practices

1. **Keep hooks focused** - One purpose per hook
2. **Use subdirectories** - Group related hooks
3. **Handle errors gracefully** - Don't break Claude's workflow
4. **Add timeouts** - Prevent hanging operations
5. **Document behavior** - Comment complex logic
6. **Test thoroughly** - Verify hooks work as expected

## Security Notes

- Hooks run with your user permissions
- Always validate input from stdin
- Be cautious with file operations
- Never store secrets in hook scripts
- Use absolute paths when possible
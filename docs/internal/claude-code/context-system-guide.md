# Context-Aware Documentation System Guide

## Overview

This guide documents the comprehensive context-aware documentation system implemented for Claude Code, designed to maintain understanding and context during development flows.

## System Components

### 1. Enhanced Memory System (CLAUDE.md)

**Location**: `/CLAUDE.md` (project root)

The CLAUDE.md file now includes documentation imports that automatically load relevant context:

```markdown
## Documentation Context
- Claude Code Documentation: @docs/claude-code/doc-index.md
- Implementation Patterns: @.claude/context/implementation-patterns.md
- Current Development Focus: @.claude/context/current-focus.md
```

These imports ensure documentation is always available in Claude's context.

### 2. Smart Hooks System

#### SessionStart Hook - Dynamic Documentation Loading
**File**: `.claude/hooks/smart_doc_loader.py`

- Loads documentation based on current working directory
- Injects relevant docs at session start
- Configurable per project area
- Truncates long files to preserve context space

#### PostToolUse Hook - Context Reminders
**File**: `.claude/hooks/context_reminder.py`

- Provides helpful reminders after file operations
- References relevant documentation based on file type
- Non-blocking (uses exit code 0)
- Customized messages for different file patterns

#### PreCompact Hook - Documentation Preservation
**File**: `.claude/hooks/preserve_docs_context.py`

- Runs before context compaction
- Preserves critical documentation references
- Extracts key patterns and headers
- Ensures documentation context survives compaction

### 3. Documentation-Aware Subagent

**File**: `.claude/agents/docs-aware-developer.md`

A specialized subagent that:
- Always checks documentation before implementing
- Follows established patterns
- Maintains context awareness
- References documentation in responses

Key features:
- Proactive pattern compliance
- Documentation-first workflow
- Quality implementation focus
- Context preservation

### 4. Documentation Organization

#### Documentation Index
**File**: `docs/claude-code/doc-index.md`

Central index of all Claude Code documentation:
- Organized by feature area
- Quick reference sections
- Essential patterns summary
- Best practices guide

#### Implementation Patterns
**File**: `.claude/context/implementation-patterns.md`

Reference for:
- Hook implementation patterns
- Subagent design patterns
- Memory organization patterns
- Context management strategies

#### Current Focus Tracking
**File**: `.claude/context/current-focus.md`

Tracks:
- Active development areas
- Implementation status
- Next steps
- Key file locations

## Configuration

### Hook Settings
**File**: `.claude/settings.json`

All hooks are configured in the settings file:

```json
{
  "hooks": {
    "SessionStart": [...],
    "PostToolUse": [...],
    "PreCompact": [...]
  }
}
```

### Testing Hooks

To test individual hooks:

```bash
# Test SessionStart hook
echo '{"session_id": "test", "cwd": "'"$PWD"'", "source": "test"}' | python3 .claude/hooks/smart_doc_loader.py

# Test context reminder
echo '{"tool_name": "Edit", "tool_input": {"file_path": "test.py"}}' | python3 .claude/hooks/context_reminder.py

# Test preservation hook
echo '{"trigger": "manual"}' | python3 .claude/hooks/preserve_docs_context.py
```

## Usage Patterns

### 1. Starting a New Session

When you start Claude Code:
1. SessionStart hook loads relevant documentation
2. CLAUDE.md imports provide base context
3. Documentation is immediately available

### 2. During Development

While coding:
1. PostToolUse hooks provide context reminders
2. Documentation references appear after edits
3. Patterns are reinforced through reminders

### 3. Using the Subagent

Invoke the documentation-aware developer:
```
> Use the docs-aware-developer subagent to implement the new feature
```

### 4. Context Compaction

When context fills:
1. PreCompact hook preserves documentation references
2. Critical patterns are maintained
3. Documentation paths remain accessible

## Best Practices

### 1. Keep Documentation Current
- Update `.claude/context/current-focus.md` regularly
- Add new patterns to `implementation-patterns.md`
- Keep doc-index.md synchronized with new guides

### 2. Customize for Your Project
- Modify `smart_doc_loader.py` mappings for your directories
- Add project-specific reminders in `context_reminder.py`
- Create specialized subagents for your workflow

### 3. Monitor Hook Performance
- Check hook execution with `claude --debug`
- Adjust timeouts if needed
- Disable non-essential hooks if performance degrades

## Troubleshooting

### Hooks Not Running
1. Check file permissions (must be executable)
2. Verify settings.json syntax
3. Use `/hooks` command to review configuration
4. Check paths use `$CLAUDE_PROJECT_DIR`

### Documentation Not Loading
1. Verify file paths in CLAUDE.md imports
2. Check smart_doc_loader.py mappings
3. Ensure documentation files exist
4. Review SessionStart hook output

### Context Overflow
1. Adjust documentation truncation in smart_doc_loader.py
2. Reduce number of imported files
3. Use more selective documentation loading
4. Rely on PreCompact preservation

## Extension Points

### Adding New Documentation Areas

1. Update `smart_doc_loader.py` with new mappings
2. Add imports to CLAUDE.md
3. Create area-specific subagents
4. Add targeted context reminders

### Creating Project-Specific Hooks

1. Add hooks to `.claude/hooks/` directory
2. Configure in `.claude/settings.json`
3. Use project-relative paths
4. Test thoroughly before deployment

## Summary

This context-aware documentation system ensures:
- Documentation is always available when needed
- Patterns are reinforced during development
- Context is preserved across sessions
- Knowledge is maintained during long workflows

The system is fully extensible and can be customized for any project's specific needs.
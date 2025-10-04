# Context-Aware System Quick Reference

## Essential Commands

### For Users

```bash
# Start Claude with documentation context
claude

# Check current development focus
cat .claude/context/current-focus.md

# View implementation patterns
cat .claude/context/implementation-patterns.md

# See documentation index
cat docs/claude-code/doc-index.md

# Update focus (tell Claude)
> Update current-focus.md with: [new focus]

# Use specialized subagent
> Use the docs-aware-developer subagent to [task]

# Check loaded documentation
> What documentation do you have loaded?

# View hooks configuration
> /hooks

# View available subagents
> /agents
```

### For Testing

```bash
# Test SessionStart hook
echo '{"session_id":"test","cwd":"'$PWD'","source":"test"}' | python3 .claude/hooks/smart_doc_loader.py

# Test context reminder
echo '{"tool_name":"Edit","tool_input":{"file_path":"test.py"}}' | python3 .claude/hooks/context_reminder.py

# Test preservation hook
echo '{"trigger":"manual"}' | python3 .claude/hooks/preserve_docs_context.py

# Check hook permissions
ls -la .claude/hooks/*.py

# View current settings
cat .claude/settings.json | jq .
```

## File Structure

```
.claude/
├── README.md                    # Instructions for Claude
├── settings.json                # Hook configuration
├── context/
│   ├── implementation-patterns.md  # Patterns to follow
│   └── current-focus.md           # Active development
├── hooks/
│   ├── smart_doc_loader.py        # SessionStart hook
│   ├── context_reminder.py        # PostToolUse hook
│   └── preserve_docs_context.py   # PreCompact hook
└── agents/
    └── docs-aware-developer.md    # Specialized subagent

docs/
├── context-aware-system.md        # User guide
├── context-system-quick-ref.md    # This file
└── claude-code/
    ├── doc-index.md               # Documentation index
    └── context-system-guide.md    # Full system guide
```

## Hook Events & Purpose

| Hook | Event | Purpose | Exit Codes |
|------|-------|---------|------------|
| smart_doc_loader.py | SessionStart | Load docs at startup | 0 = success |
| context_reminder.py | PostToolUse | Remind about patterns | 0 = success |
| preserve_docs_context.py | PreCompact | Save docs during compaction | 0 = success |

## Key Patterns

### Hook Implementation
```python
# Success (non-blocking)
sys.exit(0)

# Block operation with feedback
sys.stderr.write("Error message")
sys.exit(2)

# JSON output for fine control
output = {
    "hookSpecificOutput": {
        "additionalContext": "context"
    }
}
print(json.dumps(output))
```

### CLAUDE.md Imports
```markdown
## Documentation Context
- Guide: @docs/my-guide.md
- Patterns: @.claude/context/patterns.md
- Max depth: 5 levels
- No circular imports
```

### Subagent Definition
```markdown
---
name: my-agent
description: When to use this agent
tools: Read, Write, Edit  # Optional
---

System prompt here
```

## Common Tasks

### Add New Documentation Area

1. Create doc file:
   ```bash
   echo "# My Guide" > docs/my-area.md
   ```

2. Add to CLAUDE.md:
   ```markdown
   - My Area: @docs/my-area.md
   ```

3. Update doc index:
   ```markdown
   - [My Area](./my-area.md)
   ```

### Create Project-Specific Hook

1. Create hook script:
   ```bash
   touch .claude/hooks/my_hook.py
   chmod +x .claude/hooks/my_hook.py
   ```

2. Add to settings.json:
   ```json
   {
     "hooks": {
       "EventName": [{
         "matcher": "ToolName",
         "hooks": [{
           "type": "command",
           "command": "$CLAUDE_PROJECT_DIR/.claude/hooks/my_hook.py"
         }]
       }]
     }
   }
   ```

### Update Development Focus

Tell Claude:
```
> Update .claude/context/current-focus.md:
> - Now working on: [feature]
> - Priority: [what's important]
> - Next steps: [what's coming]
```

## Troubleshooting Checklist

- [ ] Scripts executable? `chmod +x .claude/hooks/*.py`
- [ ] Paths use `$CLAUDE_PROJECT_DIR`?
- [ ] Files exist at import paths?
- [ ] JSON syntax valid in settings.json?
- [ ] Hooks registered? Check with `/hooks`
- [ ] Documentation files present?
- [ ] CLAUDE.md has correct imports?
- [ ] Subagent files have correct frontmatter?

## Pattern Reminders

### When You See This
```
💭 Context reminder for [file]:
  📚 [Documentation reference]
  🔍 [Key pattern]
```

### It Means
- Claude is aware of relevant patterns
- Documentation is being referenced
- Patterns are being applied

### You Should
- Review the suggestions
- Ensure Claude follows them
- Update patterns if needed

## Quick Wins

### Force Documentation Reload
```
> Re-read the implementation patterns from .claude/context/implementation-patterns.md
```

### Ensure Pattern Compliance
```
> Make sure this follows our patterns in implementation-patterns.md
```

### Get Documentation Summary
```
> Summarize the documentation you have loaded
```

### Check Subagent Understanding
```
> What does the docs-aware-developer subagent do?
```

## Environment Variables

| Variable | Purpose | Used By |
|----------|---------|---------|
| `$CLAUDE_PROJECT_DIR` | Project root path | All hooks |
| `$PWD` | Current directory | smart_doc_loader.py |

## File Permissions

```bash
# Required permissions
.claude/hooks/*.py     # 755 (executable)
.claude/context/*.md   # 644 (readable)
.claude/agents/*.md    # 644 (readable)
.claude/settings.json  # 644 (readable)
```

## Success Indicators

✅ Documentation loads at session start
✅ Reminders appear after edits
✅ Patterns are referenced in responses
✅ Subagents available via `/agents`
✅ Context preserved during compaction
✅ Claude cites specific documentation

## Need Help?

1. Check the full guide: `docs/context-aware-system.md`
2. Review system documentation: `docs/claude-code/context-system-guide.md`
3. Test hooks manually using commands above
4. Check Claude's understanding: "What patterns should you follow?"
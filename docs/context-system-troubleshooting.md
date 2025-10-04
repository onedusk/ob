# Context-Aware System Troubleshooting Guide

## Common Issues and Solutions

### 1. Documentation Not Loading at Session Start

#### Symptoms
- No "DOCUMENTATION CONTEXT LOADED" message
- Claude doesn't reference documentation
- Patterns aren't being followed

#### Diagnosis
```bash
# Test the SessionStart hook manually
echo '{"session_id":"test","cwd":"'$PWD'","source":"startup"}' | python3 .claude/hooks/smart_doc_loader.py

# Check for errors
claude --debug
```

#### Solutions

**Solution 1: Check Hook Registration**
```bash
# In Claude Code
/hooks

# Look for SessionStart entry
# Should show: smart_doc_loader.py
```

**Solution 2: Verify File Permissions**
```bash
# Make hook executable
chmod +x .claude/hooks/smart_doc_loader.py

# Verify
ls -la .claude/hooks/smart_doc_loader.py
# Should show: -rwxr-xr-x
```

**Solution 3: Fix Path Issues**
Edit `.claude/settings.json`:
```json
{
  "hooks": {
    "SessionStart": [{
      "hooks": [{
        "type": "command",
        "command": "$CLAUDE_PROJECT_DIR/.claude/hooks/smart_doc_loader.py"
      }]
    }]
  }
}
```

**Solution 4: Check Python Dependencies**
```bash
# Ensure Python 3 is available
python3 --version

# Test import requirements
python3 -c "import json, sys, os, pathlib"
```

### 2. Context Reminders Not Appearing

#### Symptoms
- No reminders after editing files
- Missing pattern suggestions
- No "Context reminder" messages

#### Diagnosis
```bash
# Test PostToolUse hook
echo '{"tool_name":"Edit","tool_input":{"file_path":"test.py"}}' | python3 .claude/hooks/context_reminder.py
```

#### Solutions

**Solution 1: Check Matcher Pattern**
In `.claude/settings.json`:
```json
{
  "PostToolUse": [{
    "matcher": "Write|Edit|MultiEdit",  // Ensure all edit tools covered
    "hooks": [{
      "command": "$CLAUDE_PROJECT_DIR/.claude/hooks/context_reminder.py"
    }]
  }]
}
```

**Solution 2: Debug Hook Execution**
```bash
# Start with debug mode
claude --debug

# Edit a file and watch for:
# [DEBUG] Executing hook: context_reminder.py
```

### 3. Documentation Not Preserved During Compaction

#### Symptoms
- Lost context after "context window full"
- Documentation references disappear
- Need to re-explain patterns

#### Diagnosis
```bash
# Test PreCompact hook
echo '{"trigger":"manual","custom_instructions":""}' | python3 .claude/hooks/preserve_docs_context.py
```

#### Solutions

**Solution 1: Ensure Hook Is Configured**
```json
{
  "PreCompact": [{
    "matcher": "*",
    "hooks": [{
      "command": "$CLAUDE_PROJECT_DIR/.claude/hooks/preserve_docs_context.py"
    }]
  }]
}
```

**Solution 2: Check File Existence**
```bash
# Verify preservation targets exist
ls -la .claude/context/
ls -la docs/claude-code/
```

### 4. CLAUDE.md Imports Not Working

#### Symptoms
- @imports not loading files
- Documentation not available despite imports
- "File not found" errors

#### Diagnosis
```bash
# Check import syntax
grep "@" CLAUDE.md

# Verify target files exist
# For each @path/to/file
ls -la path/to/file
```

#### Solutions

**Solution 1: Fix Import Paths**
```markdown
# Correct format (no quotes, no backticks)
- Documentation: @docs/guide.md

# Wrong formats
- Documentation: "@docs/guide.md"  # No quotes
- Documentation: `@docs/guide.md`  # No backticks
```

**Solution 2: Check Import Depth**
- Maximum 5 levels of recursive imports
- Avoid circular dependencies

**Solution 3: Use Correct Relative Paths**
```markdown
# From project root
@docs/guide.md
@.claude/context/patterns.md

# From home directory
@~/.claude/personal.md
```

### 5. Subagent Not Available or Working

#### Symptoms
- Subagent not listed in `/agents`
- "Subagent not found" errors
- Subagent not following instructions

#### Diagnosis
```bash
# Check subagent file
ls -la .claude/agents/

# Verify frontmatter
head -n 10 .claude/agents/docs-aware-developer.md
```

#### Solutions

**Solution 1: Fix Frontmatter Format**
```markdown
---
name: docs-aware-developer
description: Clear description of when to use
tools: Read, Write, Edit  # Optional, comma-separated
---

System prompt here
```

**Solution 2: Fix Name Format**
- Use lowercase letters and hyphens only
- No spaces or underscores
- Example: `docs-aware-developer` ✓
- Wrong: `Docs_Aware_Developer` ✗

**Solution 3: Check Tool Names**
```markdown
# Correct tool names (case-sensitive)
tools: Read, Write, Edit, Bash, Grep, Glob

# Wrong
tools: read, write, edit  # Must be capitalized
```

### 6. Hooks Running But Not Working

#### Symptoms
- Hooks execute but don't affect behavior
- No errors but no effect
- Output not visible

#### Diagnosis
```bash
# Check exit codes
echo '{"test":"data"}' | python3 .claude/hooks/your_hook.py
echo $?  # Should be 0 for success, 2 for blocking
```

#### Solutions

**Solution 1: Fix Exit Codes**
```python
# Non-blocking success
sys.exit(0)

# Blocking with feedback (PreToolUse only)
sys.stderr.write("Feedback for Claude")
sys.exit(2)
```

**Solution 2: Use Correct Output Stream**
```python
# For SessionStart - stdout adds context
print("Additional context")  # or print(json.dumps({...}))

# For blocking feedback - use stderr
sys.stderr.write("Error message")
```

### 7. Performance Issues

#### Symptoms
- Slow session starts
- Delays after edits
- Timeouts on hooks

#### Solutions

**Solution 1: Adjust Timeouts**
```json
{
  "hooks": [{
    "command": "...",
    "timeout": 10000  // Increase timeout (milliseconds)
  }]
}
```

**Solution 2: Optimize Documentation Loading**
Edit `smart_doc_loader.py`:
```python
# Limit lines loaded
lines = content.split('\n')[:100]  # Reduce from 100 if needed
```

**Solution 3: Disable Non-Essential Hooks**
Temporarily comment out in settings.json:
```json
{
  "PostToolUse": [
    // {
    //   "matcher": "Write|Edit",
    //   "hooks": [...]
    // }
  ]
}
```

### 8. Claude Not Following Patterns

#### Symptoms
- Ignoring documented patterns
- Not referencing documentation
- Inconsistent implementations

#### Solutions

**Solution 1: Explicit Pattern Reference**
```
> Follow the patterns in .claude/context/implementation-patterns.md exactly
```

**Solution 2: Use Documentation Subagent**
```
> Use the docs-aware-developer subagent for this implementation
```

**Solution 3: Reload Documentation**
```
> Re-read implementation-patterns.md and current-focus.md
```

## Diagnostic Commands

### Complete System Check

```bash
# 1. Check file structure
echo "=== File Structure ==="
ls -la .claude/
ls -la .claude/hooks/
ls -la .claude/agents/
ls -la .claude/context/

# 2. Verify executables
echo "=== Executable Status ==="
ls -la .claude/hooks/*.py

# 3. Check settings
echo "=== Settings Configuration ==="
cat .claude/settings.json | python3 -m json.tool

# 4. Test each hook
echo "=== Hook Tests ==="
echo '{"session_id":"test","cwd":"'$PWD'","source":"test"}' | python3 .claude/hooks/smart_doc_loader.py
echo '{"tool_name":"Edit","tool_input":{"file_path":"test.py"}}' | python3 .claude/hooks/context_reminder.py
echo '{"trigger":"manual"}' | python3 .claude/hooks/preserve_docs_context.py

# 5. Check imports
echo "=== CLAUDE.md Imports ==="
grep "@" CLAUDE.md

# 6. Verify documentation exists
echo "=== Documentation Files ==="
ls -la docs/claude-code/
```

### Quick Health Check

```bash
# One-liner health check
[ -x .claude/hooks/smart_doc_loader.py ] && \
[ -f .claude/context/implementation-patterns.md ] && \
[ -f .claude/settings.json ] && \
echo "✅ System OK" || echo "❌ System needs attention"
```

## Error Messages and Meanings

| Error Message | Meaning | Solution |
|--------------|---------|----------|
| "No such file or directory" | Hook script not found | Check path in settings.json |
| "Permission denied" | Hook not executable | Run `chmod +x` on script |
| "json.decoder.JSONDecodeError" | Invalid JSON in settings | Validate JSON syntax |
| "ModuleNotFoundError" | Python import missing | Install required modules |
| "timeout" | Hook exceeded time limit | Increase timeout in settings |
| "File has not been read yet" | Import target missing | Create missing file |
| "maximum recursion depth" | Circular imports | Remove circular dependencies |

## Prevention Strategies

### 1. Regular Validation

Create a validation script:
```bash
#!/bin/bash
# save as .claude/validate.sh

echo "Validating Context System..."

# Check hooks
for hook in .claude/hooks/*.py; do
  if [ -x "$hook" ]; then
    echo "✅ $hook is executable"
  else
    echo "❌ $hook needs chmod +x"
  fi
done

# Check JSON
python3 -m json.tool < .claude/settings.json > /dev/null 2>&1
if [ $? -eq 0 ]; then
  echo "✅ settings.json is valid"
else
  echo "❌ settings.json has invalid JSON"
fi

# Check imports
for import in $(grep -o "@[^[:space:]]*" CLAUDE.md); do
  file="${import:1}"  # Remove @
  if [ -f "$file" ]; then
    echo "✅ Import $file exists"
  else
    echo "❌ Import $file missing"
  fi
done
```

### 2. Backup Configuration

```bash
# Backup working configuration
cp -r .claude .claude.backup

# Restore if needed
cp -r .claude.backup .claude
```

### 3. Test Before Committing

```bash
# Add to pre-commit hook
python3 -m json.tool < .claude/settings.json || exit 1
for hook in .claude/hooks/*.py; do
  python3 -m py_compile "$hook" || exit 1
done
```

## Getting Help

If issues persist after trying these solutions:

1. **Enable Debug Mode**
   ```bash
   claude --debug > debug.log 2>&1
   ```

2. **Collect Diagnostic Information**
   - Run the complete system check above
   - Save output to a file
   - Note specific error messages

3. **Check Documentation**
   - Review `docs/context-aware-system.md`
   - Check `docs/claude-code/context-system-guide.md`
   - Consult quick reference: `docs/context-system-quick-ref.md`

4. **Test Minimal Configuration**
   - Temporarily simplify settings.json
   - Test with a single hook
   - Gradually add complexity

Remember: Most issues are related to file permissions, paths, or JSON syntax. Check these first!
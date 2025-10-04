# Context-Aware Documentation System User Guide

## What Is This?

The Context-Aware Documentation System is an intelligent framework that ensures Claude Code always has access to relevant documentation and follows established patterns during development. It solves the common problem of AI assistants losing context or forgetting important project patterns during long sessions.

## Why Use This System?

### Problems It Solves

1. **Context Loss** - AI assistants forget documentation during long sessions
2. **Pattern Drift** - Code deviates from established patterns over time
3. **Manual Reminders** - Constantly having to remind the AI about project conventions
4. **Documentation Discovery** - AI doesn't know which docs are relevant
5. **Context Overflow** - Important information gets lost during compaction

### Benefits You Get

- **Consistent Code** - Claude always follows your documented patterns
- **Faster Development** - No need to repeatedly explain conventions
- **Automatic Guidance** - Relevant docs load based on what you're working on
- **Preserved Knowledge** - Critical documentation survives context resets
- **Smart Assistance** - Specialized subagents for complex tasks

## How to Use It

### Quick Start

1. **Start Claude Code** in your project directory
   ```bash
   claude
   ```

2. **The system automatically**:
   - Loads relevant documentation at startup
   - Provides reminders after file edits
   - Preserves docs during context compaction
   - Makes specialized subagents available

3. **Just work normally** - The system operates in the background

### Key Commands

#### Check Current Documentation Context
```bash
# View what Claude is currently focused on
cat .claude/context/current-focus.md
```

#### Update Development Focus
Tell Claude about your current priorities:
```
> Update .claude/context/current-focus.md - I'm now working on [description]
```

#### Use Specialized Subagents
For complex tasks requiring strict pattern compliance:
```
> Use the docs-aware-developer subagent to implement [feature]
```

#### View Documentation Index
```
> Show me the documentation index
```
Claude will reference `docs/claude-code/doc-index.md`

### Working with the System

#### 1. Starting a New Feature

Tell Claude your intent, and the system will:
- Load relevant documentation automatically
- Apply appropriate patterns
- Provide context-aware suggestions

Example:
```
> I need to create a new hook for validating Python imports
```

Claude will:
1. Reference hook documentation
2. Apply hook implementation patterns
3. Create the hook following established conventions

#### 2. Maintaining Patterns

The system ensures consistency by:
- Reminding Claude of patterns after edits
- Loading pattern documentation at session start
- Preserving pattern references during compaction

You'll see reminders like:
```
💭 Context reminder for hooks/my_hook.py:
  📚 Hook patterns: Check docs/claude-code/hooks.md
  🔍 Exit codes: 0=success, 2=block with feedback
```

#### 3. Complex Implementations

For tasks requiring deep documentation knowledge:
```
> Use the docs-aware-developer subagent to refactor the authentication system
```

The subagent will:
- Review all relevant documentation
- Identify applicable patterns
- Implement following best practices
- Maintain documentation references

## System Components

### 1. Memory System (CLAUDE.md)

Your project's CLAUDE.md includes imports that automatically load:
- Documentation index
- Implementation patterns
- Current development focus

### 2. Smart Hooks

#### SessionStart Hook
- **What**: Loads docs when Claude starts
- **When**: Every new session
- **Benefit**: Immediate context availability

#### PostToolUse Hook
- **What**: Provides reminders after edits
- **When**: After file modifications
- **Benefit**: Reinforces patterns

#### PreCompact Hook
- **What**: Preserves docs during compaction
- **When**: Context window fills
- **Benefit**: Maintains continuity

### 3. Documentation Structure

```
project/
├── .claude/
│   ├── context/           # Project context files
│   │   ├── implementation-patterns.md
│   │   └── current-focus.md
│   ├── hooks/             # Automation scripts
│   └── agents/            # Specialized subagents
├── docs/
│   └── claude-code/       # Claude documentation
│       └── doc-index.md   # Central index
└── CLAUDE.md              # Main memory file
```

## Customization

### Adding Project-Specific Documentation

1. **Create documentation** in `docs/` directory
2. **Update the index** in `docs/claude-code/doc-index.md`
3. **Add import** to CLAUDE.md:
   ```markdown
   - My Feature Guide: @docs/my-feature.md
   ```

### Customizing Documentation Loading

Edit `.claude/hooks/smart_doc_loader.py` to add mappings:
```python
doc_mappings = {
    "src/api": {
        "primary": ["docs/api-guide.md"],
        "context": ["docs/api-patterns.md"]
    }
}
```

### Creating Project-Specific Subagents

1. Create `.claude/agents/my-specialist.md`:
   ```markdown
   ---
   name: my-specialist
   description: Specialist for [specific task]
   tools: Read, Write, Edit
   ---

   [System prompt here]
   ```

2. Use it:
   ```
   > Use the my-specialist subagent for [task]
   ```

## Best Practices

### 1. Keep Documentation Current

- Update `.claude/context/current-focus.md` when switching tasks
- Add new patterns to `implementation-patterns.md`
- Keep doc index synchronized

### 2. Use Descriptive Requests

Instead of:
```
> Fix the bug
```

Use:
```
> Fix the authentication bug following our error handling patterns
```

### 3. Leverage Subagents

For complex tasks, explicitly request specialized help:
```
> Use the docs-aware-developer subagent to ensure this follows all our patterns
```

### 4. Review Reminders

Pay attention to context reminders - they indicate which patterns Claude is following.

## Troubleshooting

### Claude Not Following Patterns

1. Check documentation is loaded:
   ```
   > What documentation do you have loaded?
   ```

2. Verify patterns file exists:
   ```bash
   cat .claude/context/implementation-patterns.md
   ```

3. Explicitly reference patterns:
   ```
   > Follow the patterns in implementation-patterns.md
   ```

### Hooks Not Working

1. Check hook configuration:
   ```
   > /hooks
   ```

2. Verify scripts are executable:
   ```bash
   ls -la .claude/hooks/*.py
   ```

3. Test manually:
   ```bash
   echo '{"test": "data"}' | python3 .claude/hooks/smart_doc_loader.py
   ```

### Documentation Not Loading

1. Check CLAUDE.md imports:
   ```bash
   grep "@" CLAUDE.md
   ```

2. Verify files exist:
   ```bash
   ls -la docs/claude-code/
   ```

3. Review SessionStart hook output:
   ```
   > What documentation was loaded at session start?
   ```

## Advanced Usage

### Conditional Documentation Loading

You can make documentation load conditionally based on:
- Working directory
- File types being edited
- Current git branch
- Environment variables

Edit `smart_doc_loader.py` to add conditions.

### Pattern Enforcement

Create PreToolUse hooks that enforce patterns:
```python
# Block edits that violate patterns
if not follows_pattern(edit):
    sys.exit(2)  # Block with feedback
```

### Documentation Chains

Use imports to create documentation hierarchies:
```markdown
# Main CLAUDE.md
@docs/index.md

# docs/index.md
@docs/patterns/index.md
@docs/guides/index.md
```

## Examples

### Example 1: Starting a New Feature

```
You: I need to add user authentication to the API

Claude: I'll implement user authentication following the patterns in implementation-patterns.md.
According to our API patterns, I should:
1. Create middleware for authentication
2. Use our standard error handling
3. Follow our naming conventions
[Implementation follows documented patterns exactly]
```

### Example 2: Complex Refactoring

```
You: Use the docs-aware-developer subagent to refactor the database layer

Claude: I'll invoke the docs-aware-developer subagent for this refactoring task.

Subagent: Reviewing documentation...
- Checking implementation-patterns.md for database patterns
- Following patterns from docs/database-guide.md
- Applying error handling from our standards
[Refactoring follows all documented patterns]
```

### Example 3: Pattern Reminder

After editing a hook file:
```
💭 Context reminder for hooks/validator.py:
  📚 Hook patterns: Check docs/claude-code/hooks.md
  🔍 Exit codes: 0=success, 2=block with feedback
  💡 JSON output provides fine-grained control
```

## Summary

The Context-Aware Documentation System ensures that Claude Code:
1. Always has access to relevant documentation
2. Follows established patterns consistently
3. Maintains context across sessions
4. Provides specialized assistance when needed

By using this system, you get more consistent, higher-quality code that follows your project's conventions without constant manual guidance.
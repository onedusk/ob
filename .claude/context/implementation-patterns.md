# Implementation Patterns Reference

## Claude Code Development Patterns

### Hook Implementation Patterns

#### Pattern 1: Validation Hooks
- Use PreToolUse with exit code 2 to block operations
- Provide clear feedback via stderr for Claude to understand
- Return JSON for fine-grained control

#### Pattern 2: Context Loading
- SessionStart hooks can inject documentation
- UserPromptSubmit hooks add context based on prompts
- Use `hookSpecificOutput.additionalContext` for structured injection

#### Pattern 3: Automated Formatting
- PostToolUse hooks for code formatting
- Check file extensions before processing
- Use project-relative paths with $CLAUDE_PROJECT_DIR

### Subagent Design Patterns

#### Pattern 1: Focused Specialists
- Single responsibility principle
- Limited tool access for safety
- Detailed system prompts with examples

#### Pattern 2: Review Agents
- Read-only tools (Read, Grep, Glob)
- Structured output format
- Proactive invocation with "USE PROACTIVELY" in description

#### Pattern 3: Development Agents
- Full tool access for implementation
- Include testing in workflow
- Context-aware with documentation references

### Memory Organization Patterns

#### Pattern 1: Hierarchical Structure
```
CLAUDE.md (root)
├── @docs/index.md
├── @.claude/context/patterns.md
└── @~/.claude/personal-prefs.md
```

#### Pattern 2: Dynamic Imports
- Use conditional imports based on directory
- Maximum 5 levels of recursive imports
- Avoid circular dependencies

### Context Management Strategies

#### Strategy 1: Layered Context
1. Enterprise policies (base layer)
2. Project documentation (team layer)
3. User preferences (personal layer)
4. Session-specific context (dynamic layer)

#### Strategy 2: Just-In-Time Loading
- Load documentation when entering directories
- Use hooks to detect context switches
- Preserve critical context during compaction

## Best Practices

### Documentation References
1. Always reference documentation files by relative path
2. Use @imports in CLAUDE.md for automatic loading
3. Create focused documentation files for specific domains

### Hook Safety
1. Validate all inputs from stdin
2. Use timeouts for long-running operations
3. Handle errors gracefully with meaningful messages

### Subagent Effectiveness
1. Write clear, actionable descriptions
2. Include workflow steps in system prompts
3. Test with various edge cases

### Performance Optimization
1. Minimize hook execution time
2. Cache frequently accessed documentation
3. Use parallel execution where possible
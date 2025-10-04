# Claude Code Documentation Index

## Core Features Documentation

### Hooks System
- **Reference**: [hooks.md](./hooks.md) - Complete hooks reference with all events and configurations
- **Guide**: [hooks-guide.md](./hooks-guide.md) - Practical examples and quickstart guide
- **Key Concepts**: Event-driven automation, permission control, context injection

### Subagents
- **Main Guide**: [sub-agents.md](./sub-agents.md) - Creating and managing specialized AI subagents
- **SDK Integration**: [sdk/subagents.md](./sdk/subagents.md) - Using subagents in SDK applications
- **Benefits**: Context isolation, parallel execution, specialized expertise

### Memory Management
- **Memory Guide**: [memory.md](./memory.md) - Hierarchical memory system (CLAUDE.md files)
- **Import System**: Support for @path/to/file imports for modular organization
- **Scope**: Enterprise, Project, User, and Local project memories

### Settings & Configuration
- **Settings Reference**: [settings.md](./settings.md) - Complete settings documentation
- **Output Styles**: [output-styles.md](./output-styles.md) - Customizing Claude's output format
- **Model Configuration**: [model-config.md](./model-config.md) - Model selection and parameters

## Development Workflows

### Common Operations
- **Slash Commands**: [slash-commands.md](./slash-commands.md) - Built-in commands reference
- **Common Workflows**: [common-workflows.md](./common-workflows.md) - Typical development patterns
- **Interactive Mode**: [interactive-mode.md](./interactive-mode.md) - REPL and interactive features

### SDK Development
- **SDK Overview**: [sdk/sdk-overview.md](./sdk/sdk-overview.md) - SDK architecture and capabilities
- **Custom Tools**: [sdk/custom-tools.md](./sdk/custom-tools.md) - Creating custom tools
- **Slash Commands**: [sdk/sdk-slash-commands.md](./sdk/sdk-slash-commands.md) - Custom slash commands
- **Todo Tracking**: [sdk/todo-tracking.md](./sdk/todo-tracking.md) - Task management in SDK

## Integration & Extensions

### MCP (Model Context Protocol)
- **MCP Guide**: [mcp.md](./mcp.md) - Connecting MCP servers for extended capabilities
- **SDK MCP**: [sdk/sdk-mcp.md](./sdk/sdk-mcp.md) - MCP integration in SDK applications

### Third-Party Integrations
- **IDE Integrations**: [ide-integrations.md](./ide-integrations.md) - VS Code and other IDEs
- **Third-Party Tools**: [third-party-integrations.md](./third-party-integrations.md) - External tool integration

## Quick Reference

### Essential Patterns
1. **Hooks for Automation**: PreToolUse, PostToolUse, SessionStart, Stop events
2. **Subagents for Specialization**: Isolated context, specific tools, focused tasks
3. **Memory for Persistence**: CLAUDE.md files with imports for context loading
4. **Settings for Customization**: User and project-level configurations

### Best Practices
- Use SessionStart hooks to load context based on working directory
- Create focused subagents with limited tool access
- Organize documentation with CLAUDE.md imports
- Leverage PostToolUse hooks for validation and formatting
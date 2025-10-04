# Git Sync Hook Guide

## Overview

The Git Sync Hook automatically manages your Git staging area after file modifications in Claude Code. It follows safe Git practices by staging changes but never auto-committing, giving you full control over your commits.

## Features

- **Automatic Staging**: Stages modified files after edits
- **Safety Checks**: Skips sensitive files (.env, keys, secrets)
- **Smart Suggestions**: Provides commit message suggestions
- **Batch Tracking**: Groups related changes for better commit messages
- **Git Status Updates**: Shows repository status after changes
- **Configurable**: Fully customizable via JSON configuration

## How It Works

1. **Triggers After File Operations**: Runs after Write, Edit, or MultiEdit operations
2. **Checks Git Repository**: Only operates in Git repositories
3. **Validates Files**: Ensures files aren't sensitive before staging
4. **Stages Changes**: Automatically stages modified or new files
5. **Provides Feedback**: Shows what was staged and current repository status
6. **Suggests Commits**: After multiple changes, suggests commit messages

## Configuration

Edit `.claude/hooks/git_sync_config.json` to customize behavior:

```json
{
  "enabled": true,                    // Enable/disable the hook
  "auto_stage": true,                 // Automatically stage changes
  "stage_threshold": 1,               // Files needed before showing status
  "suggest_commit_after": 3,          // Staged files before suggesting commit
  "batch_timeout_seconds": 300,       // Time window for batching changes
  "sensitive_patterns": [...],        // Regex patterns for sensitive files
  "never_stage": [...],               // Files to never stage
  "status_emoji": {...}              // Customize emoji in messages
}
```

## Usage Examples

### Example 1: Single File Edit

When you edit a file:
```
> Update the README.md with new installation instructions
```

Hook output:
```
✅ Git: staged README.md
📊 Git Status: 📦 Staged: 1 file(s)
```

### Example 2: Multiple File Changes

After editing several files:
```
✅ Git: staged src/main.py
✅ Git: staged src/utils.py
✅ Git: staged tests/test_main.py
📊 Git Status: 📦 Staged: 3 file(s)
💡 Suggested commit: git commit -m "Update files in src"
```

### Example 3: Sensitive File Protection

When editing sensitive files:
```
⚠️ Skipped staging sensitive file: .env.local
```

## Safety Features

### Protected Files

The hook automatically skips:
- Environment files (`.env`, `.env.local`, `.env.*`)
- Private keys (`*.key`, `*.pem`, `id_rsa`)
- Secret files (`*secret*`, `credentials.json`)
- SSH configurations (`.ssh/`)
- Cloud credentials (`.aws/credentials`)

### No Auto-Commit

The hook **never** automatically commits or pushes. It only:
- Stages files for your review
- Suggests commit messages
- Shows repository status

You maintain full control over when and how to commit.

## Customization

### Disable Auto-Staging

To review changes before staging:
```json
{
  "auto_stage": false
}
```

### Adjust Commit Suggestions

Change when commit messages are suggested:
```json
{
  "suggest_commit_after": 5  // Suggest after 5 staged files
}
```

### Add Custom Sensitive Patterns

Protect additional files:
```json
{
  "sensitive_patterns": [
    ".*\\.secret$",
    "config/production\\.json",
    ".*password.*"
  ]
}
```

### Custom Emoji

Personalize status messages:
```json
{
  "status_emoji": {
    "staged": "📋",
    "success": "👍",
    "warning": "🚨"
  }
}
```

## Commands

### Check Git Status Manually
```bash
git status
```

### Review Staged Changes
```bash
git diff --cached
```

### Commit Staged Changes
```bash
git commit -m "Your message"
```

### Unstage a File
```bash
git reset HEAD file.txt
```

### Disable Hook Temporarily

Set in config:
```json
{
  "enabled": false
}
```

Or remove from `.claude/settings.json`:
```json
{
  "PostToolUse": [
    // Comment out or remove the git_sync.py entry
  ]
}
```

## Troubleshooting

### Hook Not Running

1. Check if enabled in config:
   ```bash
   cat .claude/hooks/git_sync_config.json | grep enabled
   ```

2. Verify hook is registered:
   ```
   /hooks
   ```

3. Check if in Git repository:
   ```bash
   git rev-parse --git-dir
   ```

### Files Not Staging

1. Check if file is in .gitignore:
   ```bash
   git check-ignore file.txt
   ```

2. Verify file isn't sensitive:
   ```bash
   grep file.txt .claude/hooks/git_sync_config.json
   ```

3. Check auto_stage setting:
   ```bash
   cat .claude/hooks/git_sync_config.json | grep auto_stage
   ```

### Testing the Hook

Test manually:
```bash
echo '{"tool_name":"Edit","tool_input":{"file_path":"test.txt"}}' | \
  python3 .claude/hooks/git_sync.py
```

## Best Practices

1. **Review Before Committing**: Always review staged changes before committing
2. **Meaningful Commits**: Edit suggested commit messages to be more descriptive
3. **Batch Related Changes**: Let the hook group related changes together
4. **Protect Sensitive Data**: Add patterns for any sensitive files in your project
5. **Regular Commits**: Commit frequently to maintain clear history

## Integration with Other Hooks

The Git Sync Hook works alongside other hooks:
- **Context Reminder Hook**: Provides documentation context
- **Oober Integration**: Handles bulk operations
- **Smart Doc Loader**: Maintains documentation awareness

## Performance

The hook is designed to be lightweight:
- Runs asynchronously after file operations
- 3-second timeout to prevent delays
- Caches batch information in `/tmp`
- Only processes Git-tracked directories

## Summary

The Git Sync Hook streamlines your Git workflow by:
- Automatically staging changes as you work
- Protecting sensitive files from accidental commits
- Suggesting meaningful commit messages
- Maintaining awareness of repository status

This allows you to focus on coding while the hook handles routine Git operations, ensuring a clean and organized repository history.
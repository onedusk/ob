#!/usr/bin/env python3
"""
Git Sync Hook for Claude Code
Automatically stages file changes and provides git status after CRUD operations
Follows safe git practices - stages but doesn't auto-commit
"""
import json
import sys
import os
import subprocess
from pathlib import Path
import re

# Load configuration
CONFIG_FILE = os.path.join(os.path.dirname(__file__), 'git_sync_config.json')
DEFAULT_CONFIG = {
    'enabled': True,
    'auto_stage': True,
    'stage_threshold': 1,
    'suggest_commit_after': 3,
    'batch_timeout_seconds': 300,
}

def load_config():
    """Load configuration from file or use defaults."""
    try:
        if os.path.exists(CONFIG_FILE):
            with open(CONFIG_FILE, 'r') as f:
                config = json.load(f)
                return config
    except Exception as e:
        sys.stderr.write(f"Config load error, using defaults: {e}\n")

    return DEFAULT_CONFIG

# Load configuration
config = load_config()

# Configuration values
SENSITIVE_PATTERNS = config.get('sensitive_patterns', [
    r'\.env$',
    r'\.env\.local$',
    r'\.env\.\w+$',
    r'.*\.(key|pem|p12|pfx)$',
    r'.*\.secret',
    r'.*_secret',
    r'id_rsa',
    r'id_dsa',
    r'\.ssh/',
    r'\.aws/credentials',
    r'\.npmrc$',
])

NEVER_STAGE = config.get('never_stage', [
    '.env',
    '.env.local',
    'secrets.json',
    'credentials.json',
    '.DS_Store',
    'Thumbs.db',
])

# Batch tracking for commit messages
BATCH_FILE = '/tmp/claude_git_batch.json'
BATCH_TIMEOUT = config.get('batch_timeout_seconds', 300)

def is_git_repo():
    """Check if current directory is a git repository."""
    try:
        result = subprocess.run(
            ['git', 'rev-parse', '--git-dir'],
            capture_output=True,
            text=True,
            check=False
        )
        return result.returncode == 0
    except FileNotFoundError:
        return False

def is_sensitive_file(file_path):
    """Check if file matches sensitive patterns."""
    for pattern in SENSITIVE_PATTERNS:
        if re.search(pattern, file_path, re.IGNORECASE):
            return True

    file_name = Path(file_path).name
    return file_name in NEVER_STAGE

def get_git_status():
    """Get current git status."""
    try:
        result = subprocess.run(
            ['git', 'status', '--porcelain'],
            capture_output=True,
            text=True,
            check=True
        )
        return result.stdout.strip().split('\n') if result.stdout.strip() else []
    except subprocess.CalledProcessError:
        return []

def get_file_status(file_path):
    """Get git status for a specific file."""
    try:
        result = subprocess.run(
            ['git', 'status', '--porcelain', file_path],
            capture_output=True,
            text=True,
            check=True
        )
        return result.stdout.strip()
    except subprocess.CalledProcessError:
        return ""

def stage_file(file_path):
    """Stage a file for commit."""
    try:
        subprocess.run(
            ['git', 'add', file_path],
            capture_output=True,
            check=True
        )
        return True
    except subprocess.CalledProcessError as e:
        sys.stderr.write(f"Failed to stage {file_path}: {e}\n")
        return False

def track_change(file_path, operation):
    """Track file changes for batch operations."""
    try:
        # Load existing batch
        batch_data = {'files': [], 'start_time': None}
        if os.path.exists(BATCH_FILE):
            try:
                with open(BATCH_FILE, 'r') as f:
                    batch_data = json.load(f)
            except:
                pass

        # Add current change
        import time
        current_time = time.time()

        # Reset batch if timeout
        if batch_data.get('start_time'):
            if current_time - batch_data['start_time'] > BATCH_TIMEOUT:
                batch_data = {'files': [], 'start_time': current_time}
        else:
            batch_data['start_time'] = current_time

        # Track the file
        file_entry = {
            'path': file_path,
            'operation': operation,
            'time': current_time
        }

        # Avoid duplicates
        if not any(f['path'] == file_path for f in batch_data['files']):
            batch_data['files'].append(file_entry)

        # Save batch
        with open(BATCH_FILE, 'w') as f:
            json.dump(batch_data, f)

        return batch_data
    except Exception as e:
        sys.stderr.write(f"Batch tracking error: {e}\n")
        return {'files': []}

def suggest_commit_message(batch_data):
    """Generate a suggested commit message based on changes."""
    if not batch_data.get('files'):
        return "Update files"

    files = batch_data['files']
    file_count = len(files)

    if file_count == 1:
        file_path = files[0]['path']
        file_name = Path(file_path).name
        operation = files[0]['operation']

        if operation == 'Write':
            if not os.path.exists(file_path):
                return f"Create {file_name}"
            return f"Update {file_name}"
        elif operation in ['Edit', 'MultiEdit']:
            return f"Update {file_name}"
    else:
        # Multiple files - try to find pattern
        dirs = set()
        extensions = set()

        for f in files:
            path = Path(f['path'])
            if path.parent != Path('.'):
                dirs.add(str(path.parent))
            if path.suffix:
                extensions.add(path.suffix)

        if len(dirs) == 1:
            return f"Update files in {list(dirs)[0]}"
        elif len(extensions) == 1:
            ext = list(extensions)[0]
            return f"Update {ext} files"
        else:
            return f"Update {file_count} files"

def format_status_message(staged_files, unstaged_files, untracked_files):
    """Format a helpful status message."""
    emoji = config.get('status_emoji', {})
    parts = []

    if staged_files:
        parts.append(f"{emoji.get('staged', '📦')} Staged: {len(staged_files)} file(s)")

    if unstaged_files:
        parts.append(f"{emoji.get('modified', '📝')} Modified: {len(unstaged_files)} file(s)")

    if untracked_files:
        parts.append(f"{emoji.get('untracked', '🆕')} Untracked: {len(untracked_files)} file(s)")

    return " | ".join(parts) if parts else f"{emoji.get('clean', '✨')} Working tree clean"

def main():
    """Main execution."""
    try:
        # Check if hook is enabled
        if not config.get('enabled', True):
            sys.exit(0)

        # Read input from stdin
        input_data = json.load(sys.stdin)

        # Extract information
        tool_name = input_data.get("tool_name", "")
        tool_input = input_data.get("tool_input", {})
        file_path = tool_input.get("file_path", "")

        # Only process file modification tools
        if tool_name not in ["Write", "Edit", "MultiEdit"]:
            sys.exit(0)

        # Check if we're in a git repo
        if not is_git_repo():
            sys.exit(0)  # Silently exit if not a git repo

        # Skip if no file path
        if not file_path:
            sys.exit(0)

        # Make path relative to repo root
        file_path = os.path.relpath(file_path)

        # Check if file is sensitive
        if is_sensitive_file(file_path):
            warning_emoji = config.get('status_emoji', {}).get('warning', '⚠️')
            sys.stderr.write(f"{warning_emoji}  Skipped staging sensitive file: {file_path}\n")
            sys.exit(0)

        # Get file's git status
        file_status = get_file_status(file_path)

        if not file_status:
            # File might be ignored by .gitignore
            sys.exit(0)

        status_code = file_status[:2] if file_status else ""

        # Track the change
        batch_data = track_change(file_path, tool_name)

        # Stage the file if it's modified or new (and auto_stage is enabled)
        staged = False
        if config.get('auto_stage', True) and status_code in [' M', 'MM', 'AM', '??', 'A ', ' A']:
            if stage_file(file_path):
                staged = True
                success_emoji = config.get('status_emoji', {}).get('success', '✅')
                action = "staged" if status_code != '??' else "added"
                sys.stderr.write(f"{success_emoji} Git: {action} {file_path}\n")

        # Get overall status
        all_status = get_git_status()

        # Parse status
        staged_files = []
        unstaged_files = []
        untracked_files = []

        for line in all_status:
            if not line.strip():
                continue

            status = line[:2]
            filepath = line[3:]

            if status[0] in ['M', 'A', 'D', 'R', 'C']:
                staged_files.append(filepath)
            if status[1] in ['M', 'D']:
                unstaged_files.append(filepath)
            if status == '??':
                untracked_files.append(filepath)

        # Provide status summary
        if staged or len(batch_data.get('files', [])) > config.get('stage_threshold', 1):
            emoji = config.get('status_emoji', {})
            status_msg = format_status_message(staged_files, unstaged_files, untracked_files)
            sys.stderr.write(f"{emoji.get('status', '📊')} Git Status: {status_msg}\n")

            # Suggest commit if we have enough staged files
            suggest_after = config.get('suggest_commit_after', 3)
            if staged_files and len(staged_files) >= suggest_after:
                suggested_msg = suggest_commit_message(batch_data)
                sys.stderr.write(f"{emoji.get('info', '💡')} Suggested commit: git commit -m \"{suggested_msg}\"\n")

        # Always exit successfully (non-blocking)
        sys.exit(0)

    except Exception as e:
        # Log error but don't block operation
        sys.stderr.write(f"Git sync error: {e}\n")
        sys.exit(0)

if __name__ == "__main__":
    main()
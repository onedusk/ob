#!/usr/bin/env python3
"""
Command Validator
Validates shell commands for safety and best practices
"""
import re
import shlex
from typing import Dict, Any, Optional, List, Tuple
from base_validator import BaseValidator, ValidationResult

class CommandValidator(BaseValidator):
    """Validates shell commands for safety and correctness."""

    def __init__(self, config: Optional[Dict[str, Any]] = None):
        super().__init__(config)
        self.block_dangerous = self.config.get('block_dangerous', True)
        self.suggest_alternatives = self.config.get('suggest_alternatives', True)
        self.max_command_length = self.config.get('max_command_length', 5000)
        self.allowed_commands = self.config.get('allowed_commands', [])
        self.blocked_commands = self.config.get('blocked_commands', [])

    @property
    def name(self) -> str:
        return "CommandValidator"

    @property
    def description(self) -> str:
        return "Validates shell commands for safety"

    def validate(self, data: Dict[str, Any]) -> ValidationResult:
        """Validate shell command."""
        tool_name = data.get('tool_name', '')

        # Only validate Bash tool
        if tool_name != 'Bash':
            return ValidationResult(True, "Not a shell command")

        tool_input = data.get('tool_input', {})
        command = tool_input.get('command', '')

        if not command:
            return ValidationResult(True, "No command to validate")

        # Check command length
        if len(command) > self.max_command_length:
            return ValidationResult(
                False,
                f"Command too long: {len(command)} chars (max: {self.max_command_length})",
                'error'
            )

        # Parse command
        try:
            tokens = shlex.split(command)
            if not tokens:
                return ValidationResult(True, "Empty command")
            base_command = tokens[0]
        except Exception as e:
            return ValidationResult(
                False,
                f"Invalid command syntax: {e}",
                'error'
            )

        # Check blocked commands
        if self.blocked_commands and base_command in self.blocked_commands:
            return ValidationResult(
                False,
                f"Command '{base_command}' is blocked",
                'error'
            )

        # Check allowed commands (if configured)
        if self.allowed_commands and base_command not in self.allowed_commands:
            return ValidationResult(
                False,
                f"Command '{base_command}' not in allowlist",
                'error'
            )

        # Validate dangerous operations
        if self.block_dangerous:
            danger_check = self._check_dangerous_command(command, tokens)
            if not danger_check:
                return danger_check

        # Check for common mistakes
        mistake_check = self._check_common_mistakes(command, tokens)
        if mistake_check.severity == 'warning':
            return mistake_check

        # Suggest better alternatives
        if self.suggest_alternatives:
            suggestion = self._suggest_alternative(command, base_command)
            if suggestion:
                return ValidationResult(
                    True,
                    f"Suggestion: {suggestion}",
                    'info'
                )

        return ValidationResult(True, "Command validation passed")

    def _check_dangerous_command(self, command: str, tokens: List[str]) -> ValidationResult:
        """Check for dangerous commands and patterns."""
        base_command = tokens[0] if tokens else ""

        # Extremely dangerous commands
        if base_command in ['rm', 'rmdir']:
            # Check for recursive root deletion
            if '-rf' in tokens and ('/' in tokens or '/*' in command):
                return ValidationResult(
                    False,
                    "BLOCKED: Recursive deletion on root or root-level directory",
                    'error'
                )

            # Check for home directory deletion
            if '-rf' in tokens and ('~' in tokens or '$HOME' in command):
                return ValidationResult(
                    False,
                    "BLOCKED: Recursive deletion of home directory",
                    'error'
                )

            # Warn about force flag
            if '-f' in tokens:
                return ValidationResult(
                    True,
                    "Warning: Using force flag with rm",
                    'warning'
                )

        # Fork bombs
        if re.search(r':\(\)\{.*:\|:&\}', command) or ':(){ :|:& }' in command:
            return ValidationResult(
                False,
                "BLOCKED: Fork bomb detected",
                'error'
            )

        # DD command (disk operations)
        if base_command == 'dd':
            if 'of=/dev/' in command:
                return ValidationResult(
                    False,
                    "BLOCKED: DD writing to device",
                    'error'
                )

        # Chmod with dangerous permissions
        if base_command == 'chmod':
            if '777' in tokens or '666' in tokens:
                return ValidationResult(
                    True,
                    "Warning: Setting overly permissive file permissions",
                    'warning'
                )

        # Executing downloaded scripts
        dangerous_patterns = [
            (r'curl.*\|\s*(bash|sh)', "Piping curl directly to shell"),
            (r'wget.*\|\s*(bash|sh)', "Piping wget directly to shell"),
            (r'eval\s*\$?\(', "Using eval with command substitution"),
            (r'>\s*/dev/(sda|sdb|nvme|disk)', "Writing to disk device"),
        ]

        for pattern, description in dangerous_patterns:
            if re.search(pattern, command, re.IGNORECASE):
                return ValidationResult(
                    False,
                    f"BLOCKED: {description}",
                    'error'
                )

        return ValidationResult(True, "No dangerous patterns detected")

    def _check_common_mistakes(self, command: str, tokens: List[str]) -> ValidationResult:
        """Check for common command mistakes."""
        base_command = tokens[0] if tokens else ""

        # Git commands with potential issues
        if base_command == 'git':
            if len(tokens) > 1:
                git_command = tokens[1]

                # Force push to main/master
                if git_command == 'push' and '-f' in tokens:
                    if 'main' in command or 'master' in command:
                        return ValidationResult(
                            True,
                            "Warning: Force pushing to main/master branch",
                            'warning'
                        )

                # Reset --hard without specifying commit
                if git_command == 'reset' and '--hard' in tokens:
                    if len(tokens) == 3:  # git reset --hard (no commit specified)
                        return ValidationResult(
                            True,
                            "Warning: Hard reset without specifying commit",
                            'warning'
                        )

        # Find without -print0 when using with xargs
        if base_command == 'find' and '| xargs' in command and '-print0' not in command:
            return ValidationResult(
                True,
                "Tip: Use 'find -print0 | xargs -0' for filenames with spaces",
                'info'
            )

        # Grep without -F for fixed strings
        if base_command == 'grep':
            # Check if pattern looks like fixed string
            if len(tokens) > 1 and not re.search(r'[.*+?{}()\[\]|\\]', tokens[1]):
                if '-F' not in tokens:
                    return ValidationResult(
                        True,
                        "Tip: Use 'grep -F' for fixed string search (faster)",
                        'info'
                    )

        return ValidationResult(True, "No common mistakes detected")

    def _suggest_alternative(self, command: str, base_command: str) -> Optional[str]:
        """Suggest better command alternatives."""
        alternatives = {
            'cat': {
                'condition': lambda cmd: '| grep' in cmd,
                'suggestion': "Consider using 'grep' directly on the file"
            },
            'ls': {
                'condition': lambda cmd: '| grep' in cmd,
                'suggestion': "Consider using 'find' or 'ls pattern*'"
            },
            'grep': {
                'condition': lambda cmd: 'grep' in cmd and 'rg' not in cmd,
                'suggestion': "Consider using 'rg' (ripgrep) for better performance"
            },
            'find': {
                'condition': lambda cmd: '-name' in cmd and 'fd' not in cmd,
                'suggestion': "Consider using 'fd' for simpler syntax and better performance"
            },
            'cat': {
                'condition': lambda cmd: '| head' in cmd or '| tail' in cmd,
                'suggestion': "Consider using 'head' or 'tail' directly on the file"
            }
        }

        if base_command in alternatives:
            alt = alternatives[base_command]
            if alt['condition'](command):
                return alt['suggestion']

        return None
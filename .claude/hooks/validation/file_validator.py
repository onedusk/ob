#!/usr/bin/env python3
"""
File Path Validator
Validates file operations for security and safety
"""
import os
import re
from pathlib import Path
from typing import Dict, Any, Optional
from base_validator import BaseValidator, ValidationResult

class FilePathValidator(BaseValidator):
    """Validates file paths for security and safety concerns."""

    def __init__(self, config: Optional[Dict[str, Any]] = None):
        super().__init__(config)
        self.protected_dirs = self.config.get('protected_directories', [
            '/etc',
            '/usr',
            '/System',
            '/Windows/System32',
            '/.git/objects',
            '/.git/hooks'
        ])
        self.protected_files = self.config.get('protected_files', [
            '.git/config',
            '.ssh/id_rsa',
            '.ssh/id_dsa',
            '.ssh/id_ecdsa',
            '.ssh/id_ed25519',
            '.gnupg/',
            '.aws/credentials',
            '.npmrc',
            '.pypirc'
        ])
        self.max_path_depth = self.config.get('max_path_depth', 20)
        self.allow_hidden_files = self.config.get('allow_hidden_files', True)
        self.allow_symlinks = self.config.get('allow_symlinks', False)

    @property
    def name(self) -> str:
        return "FilePathValidator"

    @property
    def description(self) -> str:
        return "Validates file paths for security and safety"

    def validate(self, data: Dict[str, Any]) -> ValidationResult:
        """Validate file path for security issues."""
        tool_input = data.get('tool_input', {})
        file_path = tool_input.get('file_path', '')

        if not file_path:
            return ValidationResult(True, "No file path to validate")

        # Check for path traversal attempts
        if '../' in file_path or '..\\' in file_path:
            return ValidationResult(
                False,
                f"Path traversal detected in: {file_path}",
                'error'
            )

        # Resolve path to absolute
        try:
            abs_path = Path(file_path).resolve()
        except Exception as e:
            return ValidationResult(
                False,
                f"Invalid file path: {file_path} - {e}",
                'error'
            )

        # Check if path is within working directory (unless absolute path given)
        if not file_path.startswith('/'):
            cwd = Path.cwd()
            try:
                abs_path.relative_to(cwd)
            except ValueError:
                return ValidationResult(
                    False,
                    f"Path escapes working directory: {file_path}",
                    'error'
                )

        # Check against protected directories
        for protected_dir in self.protected_dirs:
            if str(abs_path).startswith(protected_dir):
                return ValidationResult(
                    False,
                    f"Operation blocked: {protected_dir} is a protected directory",
                    'error'
                )

        # Check against protected files
        for protected_file in self.protected_files:
            if str(abs_path).endswith(protected_file) or protected_file in str(abs_path):
                return ValidationResult(
                    False,
                    f"Operation blocked: {protected_file} is a protected file",
                    'error'
                )

        # Check path depth
        path_parts = abs_path.parts
        if len(path_parts) > self.max_path_depth:
            return ValidationResult(
                False,
                f"Path too deep: {len(path_parts)} levels (max: {self.max_path_depth})",
                'warning'
            )

        # Check hidden files
        if not self.allow_hidden_files:
            if any(part.startswith('.') for part in path_parts[1:]):  # Skip root
                return ValidationResult(
                    False,
                    f"Hidden files not allowed: {file_path}",
                    'warning'
                )

        # Check symlinks
        if not self.allow_symlinks and abs_path.is_symlink():
            return ValidationResult(
                False,
                f"Symbolic links not allowed: {file_path}",
                'error'
            )

        # Check file name for suspicious patterns
        suspicious_patterns = [
            r'[\x00-\x1f\x7f]',  # Control characters
            r'^-',                # Starts with dash (could be command flag)
            r'\$\(',              # Command substitution
            r'`',                 # Backtick command substitution
            r'[;&|]',            # Shell operators
        ]

        file_name = abs_path.name
        for pattern in suspicious_patterns:
            if re.search(pattern, file_name):
                return ValidationResult(
                    False,
                    f"Suspicious filename pattern detected: {file_name}",
                    'warning'
                )

        return ValidationResult(True, "File path validation passed")

class FileContentValidator(BaseValidator):
    """Validates file content for security and safety concerns."""

    def __init__(self, config: Optional[Dict[str, Any]] = None):
        super().__init__(config)
        self.max_file_size = self.config.get('max_file_size', 10 * 1024 * 1024)  # 10MB
        self.binary_file_action = self.config.get('binary_file_action', 'warn')  # 'block', 'warn', 'allow'

    @property
    def name(self) -> str:
        return "FileContentValidator"

    @property
    def description(self) -> str:
        return "Validates file content for security issues"

    def validate(self, data: Dict[str, Any]) -> ValidationResult:
        """Validate file content for security issues."""
        tool_name = data.get('tool_name', '')
        tool_input = data.get('tool_input', {})

        # Only validate Write operations
        if tool_name != 'Write':
            return ValidationResult(True, "Not a write operation")

        file_path = tool_input.get('file_path', '')
        content = tool_input.get('content', '')

        if not content:
            return ValidationResult(True, "No content to validate")

        # Check content size
        content_size = len(content.encode('utf-8'))
        if content_size > self.max_file_size:
            return ValidationResult(
                False,
                f"Content too large: {content_size} bytes (max: {self.max_file_size})",
                'error'
            )

        # Check for binary content
        if self._is_binary_content(content):
            if self.binary_file_action == 'block':
                return ValidationResult(
                    False,
                    "Binary content not allowed",
                    'error'
                )
            elif self.binary_file_action == 'warn':
                return ValidationResult(
                    True,
                    "Warning: Binary content detected",
                    'warning'
                )

        # Check for sensitive patterns in content
        sensitive_patterns = [
            (r'(?i)api[_-]?key\s*=\s*["\'][\w\-]+["\']', "API key detected"),
            (r'(?i)password\s*=\s*["\'][^"\']+["\']', "Password detected"),
            (r'(?i)token\s*=\s*["\'][\w\-]+["\']', "Token detected"),
            (r'-----BEGIN (RSA |DSA |EC |OPENSSH )?PRIVATE KEY-----', "Private key detected"),
            (r'[a-zA-Z0-9+/]{40,}={0,2}', "Possible base64 encoded secret")
        ]

        for pattern, message in sensitive_patterns:
            if re.search(pattern, content):
                return ValidationResult(
                    True,
                    f"Warning: {message} in content",
                    'warning'
                )

        return ValidationResult(True, "File content validation passed")

    def _is_binary_content(self, content: str) -> bool:
        """Check if content appears to be binary."""
        try:
            # Check for null bytes
            if '\x00' in content:
                return True

            # Check for high ratio of non-printable characters
            non_printable = sum(1 for c in content[:1000] if ord(c) < 32 and c not in '\n\r\t')
            if non_printable > 100:
                return True

            return False
        except Exception:
            return False
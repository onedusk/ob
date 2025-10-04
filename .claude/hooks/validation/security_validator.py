#!/usr/bin/env python3
"""
Security Validator
Validates operations for security risks and sensitive data exposure
"""
import re
import json
from typing import Dict, Any, Optional, List, Tuple
from pathlib import Path
from base_validator import BaseValidator, ValidationResult

class SecurityValidator(BaseValidator):
    """Validates operations for security risks."""

    def __init__(self, config: Optional[Dict[str, Any]] = None):
        super().__init__(config)
        self.scan_for_secrets = self.config.get('scan_for_secrets', True)
        self.block_on_secrets = self.config.get('block_on_secrets', False)
        self.allowed_domains = self.config.get('allowed_domains', [])
        self.blocked_domains = self.config.get('blocked_domains', [])
        self._load_secret_patterns()

    def _load_secret_patterns(self):
        """Load patterns for detecting secrets."""
        self.secret_patterns: List[Tuple[str, str, str]] = [
            # (pattern, description, severity)
            # API Keys
            (r'(?i)api[_-]?key[\s]*[:=][\s]*["\']?([a-zA-Z0-9\-_]{20,})["\']?', 'API Key', 'high'),
            (r'(?i)apikey[\s]*[:=][\s]*["\']?([a-zA-Z0-9\-_]{20,})["\']?', 'API Key', 'high'),

            # AWS
            (r'AKIA[0-9A-Z]{16}', 'AWS Access Key ID', 'critical'),
            (r'(?i)aws[_-]?secret[_-]?access[_-]?key[\s]*[:=][\s]*["\']?([a-zA-Z0-9+/]{40})["\']?', 'AWS Secret Key', 'critical'),

            # Private Keys
            (r'-----BEGIN (RSA |DSA |EC |OPENSSH )?PRIVATE KEY-----', 'Private Key', 'critical'),
            (r'-----BEGIN PGP PRIVATE KEY BLOCK-----', 'PGP Private Key', 'critical'),

            # GitHub
            (r'ghp_[a-zA-Z0-9]{36}', 'GitHub Personal Access Token', 'high'),
            (r'gho_[a-zA-Z0-9]{36}', 'GitHub OAuth Access Token', 'high'),
            (r'ghu_[a-zA-Z0-9]{36}', 'GitHub User Access Token', 'high'),
            (r'ghs_[a-zA-Z0-9]{36}', 'GitHub Secret', 'high'),
            (r'ghr_[a-zA-Z0-9]{36}', 'GitHub Refresh Token', 'high'),

            # Generic Passwords
            (r'(?i)password[\s]*[:=][\s]*["\']([^"\']{8,})["\']', 'Password', 'high'),
            (r'(?i)passwd[\s]*[:=][\s]*["\']([^"\']{8,})["\']', 'Password', 'high'),
            (r'(?i)pwd[\s]*[:=][\s]*["\']([^"\']{8,})["\']', 'Password', 'high'),

            # Tokens
            (r'(?i)token[\s]*[:=][\s]*["\']?([a-zA-Z0-9\-_\.]{20,})["\']?', 'Access Token', 'high'),
            (r'(?i)bearer[\s]+([a-zA-Z0-9\-_\.]{20,})', 'Bearer Token', 'high'),

            # Database URLs
            (r'(?i)(mongodb|postgres|postgresql|mysql|redis|memcached|elastic):\/\/[^:]+:[^@]+@[^\s]+', 'Database URL with credentials', 'critical'),

            # Stripe
            (r'sk_live_[a-zA-Z0-9]{24,}', 'Stripe Live Secret Key', 'critical'),
            (r'pk_live_[a-zA-Z0-9]{24,}', 'Stripe Live Public Key', 'high'),

            # Slack
            (r'xox[baprs]-[0-9]{10,13}-[a-zA-Z0-9]{24,}', 'Slack Token', 'high'),

            # Generic Secrets
            (r'(?i)secret[\s]*[:=][\s]*["\']([^"\']{8,})["\']', 'Secret Value', 'medium'),
            (r'(?i)client[_-]?secret[\s]*[:=][\s]*["\']([^"\']{8,})["\']', 'Client Secret', 'high'),
        ]

    @property
    def name(self) -> str:
        return "SecurityValidator"

    @property
    def description(self) -> str:
        return "Validates for security risks and sensitive data"

    def validate(self, data: Dict[str, Any]) -> ValidationResult:
        """Validate for security issues."""
        tool_name = data.get('tool_name', '')
        tool_input = data.get('tool_input', {})

        # Different validation based on tool type
        if tool_name == 'Bash':
            return self._validate_command(tool_input)
        elif tool_name in ['Write', 'Edit', 'MultiEdit']:
            return self._validate_file_operation(tool_input)
        elif tool_name in ['WebFetch', 'WebSearch']:
            return self._validate_web_operation(tool_input)

        return ValidationResult(True, "No security validation needed")

    def _validate_command(self, tool_input: Dict[str, Any]) -> ValidationResult:
        """Validate shell commands for security risks."""
        command = tool_input.get('command', '')

        if not command:
            return ValidationResult(True, "No command to validate")

        # Check for dangerous commands
        dangerous_commands = [
            (r'\brm\s+-rf\s+/', 'Dangerous rm -rf on root'),
            (r'curl\s+.*\|\s*sh', 'Piping curl to shell'),
            (r'wget\s+.*\|\s*bash', 'Piping wget to bash'),
            (r'eval\s+', 'Using eval command'),
            (r'exec\s+', 'Using exec command'),
            (r'>\s*/dev/sda', 'Writing to disk device'),
            (r'dd\s+.*of=/dev/', 'DD to device'),
            (r'chmod\s+777', 'Setting world-writable permissions'),
            (r'sudo\s+chmod\s+-R', 'Recursive permission change with sudo'),
        ]

        for pattern, description in dangerous_commands:
            if re.search(pattern, command, re.IGNORECASE):
                return ValidationResult(
                    False,
                    f"Dangerous command detected: {description}",
                    'error'
                )

        # Check for secrets in command
        if self.scan_for_secrets:
            secret_result = self._scan_for_secrets(command)
            if secret_result:
                severity = 'error' if self.block_on_secrets else 'warning'
                return ValidationResult(
                    not self.block_on_secrets,
                    f"Secret detected in command: {secret_result}",
                    severity
                )

        return ValidationResult(True, "Command validation passed")

    def _validate_file_operation(self, tool_input: Dict[str, Any]) -> ValidationResult:
        """Validate file operations for sensitive data."""
        file_path = tool_input.get('file_path', '')
        content = tool_input.get('content', '') or tool_input.get('new_string', '')

        # Check if operating on sensitive file
        sensitive_files = [
            '.env', '.env.local', '.env.production',
            'credentials.json', 'secrets.json', 'config.json',
            '.npmrc', '.pypirc', '.netrc',
            'id_rsa', 'id_dsa', 'id_ecdsa', 'id_ed25519'
        ]

        file_name = Path(file_path).name if file_path else ''
        if file_name in sensitive_files:
            return ValidationResult(
                True,
                f"Warning: Operating on sensitive file: {file_name}",
                'warning'
            )

        # Scan content for secrets
        if content and self.scan_for_secrets:
            secret_result = self._scan_for_secrets(content)
            if secret_result:
                severity = 'error' if self.block_on_secrets else 'warning'
                return ValidationResult(
                    not self.block_on_secrets,
                    f"Secret detected in content: {secret_result}",
                    severity
                )

        return ValidationResult(True, "File operation validation passed")

    def _validate_web_operation(self, tool_input: Dict[str, Any]) -> ValidationResult:
        """Validate web operations for security."""
        url = tool_input.get('url', '') or tool_input.get('query', '')

        if not url:
            return ValidationResult(True, "No URL to validate")

        # Extract domain from URL
        import urllib.parse
        try:
            parsed = urllib.parse.urlparse(url)
            domain = parsed.netloc.lower()
        except Exception:
            return ValidationResult(
                False,
                f"Invalid URL: {url}",
                'error'
            )

        # Check blocked domains
        if self.blocked_domains:
            for blocked in self.blocked_domains:
                if blocked in domain:
                    return ValidationResult(
                        False,
                        f"Blocked domain: {domain}",
                        'error'
                    )

        # Check allowed domains (if configured)
        if self.allowed_domains:
            allowed = any(allowed in domain for allowed in self.allowed_domains)
            if not allowed:
                return ValidationResult(
                    False,
                    f"Domain not in allowlist: {domain}",
                    'error'
                )

        # Check for suspicious URLs
        suspicious_patterns = [
            (r'(?i)(phishing|malware|virus)', 'Suspicious URL pattern'),
            (r'[0-9]{1,3}\.[0-9]{1,3}\.[0-9]{1,3}\.[0-9]{1,3}', 'IP address URL'),
            (r'@', 'URL contains @ (possible phishing)'),
            (r'[а-яА-Я]', 'URL contains Cyrillic characters'),
        ]

        for pattern, description in suspicious_patterns:
            if re.search(pattern, url):
                return ValidationResult(
                    True,
                    f"Warning: {description} in URL",
                    'warning'
                )

        return ValidationResult(True, "Web operation validation passed")

    def _scan_for_secrets(self, text: str) -> Optional[str]:
        """Scan text for potential secrets."""
        for pattern, description, severity in self.secret_patterns:
            match = re.search(pattern, text)
            if match:
                # Don't reveal the actual secret in the message
                return f"{description} (severity: {severity})"
        return None

class PermissionValidator(BaseValidator):
    """Validates operations against permission policies."""

    def __init__(self, config: Optional[Dict[str, Any]] = None):
        super().__init__(config)
        self.policies = self.config.get('policies', {})

    @property
    def name(self) -> str:
        return "PermissionValidator"

    @property
    def description(self) -> str:
        return "Validates operations against permission policies"

    def validate(self, data: Dict[str, Any]) -> ValidationResult:
        """Check if operation is permitted by policies."""
        tool_name = data.get('tool_name', '')

        # Check if tool is explicitly blocked
        blocked_tools = self.policies.get('blocked_tools', [])
        if tool_name in blocked_tools:
            return ValidationResult(
                False,
                f"Tool '{tool_name}' is blocked by policy",
                'error'
            )

        # Check if tool requires approval
        approval_required = self.policies.get('approval_required', [])
        if tool_name in approval_required:
            return ValidationResult(
                True,
                f"Tool '{tool_name}' requires user approval",
                'warning'
            )

        # Check time-based restrictions
        time_restrictions = self.policies.get('time_restrictions', {})
        if tool_name in time_restrictions:
            import datetime
            now = datetime.datetime.now()
            allowed_hours = time_restrictions[tool_name]

            if 'start_hour' in allowed_hours and 'end_hour' in allowed_hours:
                current_hour = now.hour
                start = allowed_hours['start_hour']
                end = allowed_hours['end_hour']

                if not (start <= current_hour < end):
                    return ValidationResult(
                        False,
                        f"Tool '{tool_name}' not allowed at this time",
                        'error'
                    )

        return ValidationResult(True, "Permission validation passed")
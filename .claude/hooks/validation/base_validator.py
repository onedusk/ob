#!/usr/bin/env python3
"""
Base Validator Class
Foundation for all validation hooks in Claude Code
"""
from abc import ABC, abstractmethod
from typing import Dict, Any, Tuple, Optional, List
import json
import sys

class ValidationResult:
    """Result of a validation check."""

    def __init__(self, is_valid: bool, message: str = "", severity: str = "error"):
        self.is_valid = is_valid
        self.message = message
        self.severity = severity  # 'error', 'warning', 'info'

    def __bool__(self):
        return self.is_valid

    def to_dict(self):
        return {
            "is_valid": self.is_valid,
            "message": self.message,
            "severity": self.severity
        }

class BaseValidator(ABC):
    """Abstract base class for all validators."""

    def __init__(self, config: Optional[Dict[str, Any]] = None):
        """Initialize validator with optional configuration."""
        self.config = config or {}
        self.enabled = self.config.get('enabled', True)

    @abstractmethod
    def validate(self, data: Dict[str, Any]) -> ValidationResult:
        """
        Perform validation on input data.

        Args:
            data: Input data from Claude Code hook

        Returns:
            ValidationResult object
        """
        pass

    @property
    @abstractmethod
    def name(self) -> str:
        """Return the name of this validator."""
        pass

    @property
    @abstractmethod
    def description(self) -> str:
        """Return a description of what this validator checks."""
        pass

    def should_validate(self, tool_name: str, file_path: str = "") -> bool:
        """
        Determine if this validator should run for given context.

        Args:
            tool_name: Name of the tool being used
            file_path: Path to file being operated on

        Returns:
            Boolean indicating if validation should occur
        """
        if not self.enabled:
            return False

        # Check if validator has tool restrictions
        allowed_tools = self.config.get('allowed_tools', [])
        if allowed_tools and tool_name not in allowed_tools:
            return False

        # Check if validator has file pattern restrictions
        patterns = self.config.get('file_patterns', [])
        if patterns and file_path:
            import re
            if not any(re.search(pattern, file_path) for pattern in patterns):
                return False

        return True

    def format_error(self, message: str) -> str:
        """Format an error message with validator context."""
        return f"[{self.name}] {message}"

    def format_warning(self, message: str) -> str:
        """Format a warning message with validator context."""
        return f"⚠️ [{self.name}] {message}"

    def format_info(self, message: str) -> str:
        """Format an info message with validator context."""
        return f"ℹ️ [{self.name}] {message}"

class CompositeValidator(BaseValidator):
    """Validator that runs multiple validators in sequence."""

    def __init__(self, validators: List[BaseValidator], config: Optional[Dict[str, Any]] = None):
        super().__init__(config)
        self.validators = validators
        self._name = config.get('name', 'CompositeValidator')
        self._description = config.get('description', 'Runs multiple validators')

    @property
    def name(self) -> str:
        return self._name

    @property
    def description(self) -> str:
        return self._description

    def validate(self, data: Dict[str, Any]) -> ValidationResult:
        """Run all validators and return combined result."""
        errors = []
        warnings = []

        tool_name = data.get('tool_name', '')
        file_path = data.get('tool_input', {}).get('file_path', '')

        for validator in self.validators:
            if validator.should_validate(tool_name, file_path):
                result = validator.validate(data)

                if not result.is_valid:
                    if result.severity == 'error':
                        errors.append(f"{validator.name}: {result.message}")
                    elif result.severity == 'warning':
                        warnings.append(f"{validator.name}: {result.message}")

        if errors:
            return ValidationResult(False, '\n'.join(errors), 'error')
        elif warnings:
            return ValidationResult(True, '\n'.join(warnings), 'warning')
        else:
            return ValidationResult(True, "All validations passed", 'info')

def load_validator_config(config_file: str = None) -> Dict[str, Any]:
    """Load validator configuration from file."""
    import os

    if not config_file:
        config_file = os.path.join(
            os.path.dirname(__file__),
            'validation_config.json'
        )

    try:
        with open(config_file, 'r') as f:
            return json.load(f)
    except (FileNotFoundError, json.JSONDecodeError):
        return {}

def handle_validation_result(result: ValidationResult, blocking: bool = True):
    """
    Handle validation result appropriately for Claude Code.

    Args:
        result: ValidationResult object
        blocking: Whether to block on validation failure
    """
    if not result.is_valid and blocking:
        # Exit code 2 blocks the operation and shows message to Claude
        sys.stderr.write(result.message + '\n')
        sys.exit(2)
    elif result.severity == 'warning':
        # Show warning but don't block
        sys.stderr.write(result.message + '\n')
        sys.exit(0)
    else:
        # Success
        sys.exit(0)
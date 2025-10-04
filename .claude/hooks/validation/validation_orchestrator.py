#!/usr/bin/env python3
"""
Validation Orchestrator
Main validation hook that coordinates all validators
"""
import json
import sys
import os
from typing import Dict, Any, List
from pathlib import Path

# Add validation directory to path
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from base_validator import BaseValidator, ValidationResult, CompositeValidator, load_validator_config, handle_validation_result
from file_validator import FilePathValidator, FileContentValidator
from security_validator import SecurityValidator, PermissionValidator
from pattern_validator import PatternValidator
from command_validator import CommandValidator

class ValidationOrchestrator:
    """Orchestrates all validation hooks."""

    def __init__(self, config_file: str = None):
        """Initialize the orchestrator with configuration."""
        self.config = load_validator_config(config_file)
        self.validators = self._initialize_validators()
        self.enabled = self.config.get('enabled', True)
        self.mode = self.config.get('mode', 'blocking')  # 'blocking', 'warning', 'silent'
        self.log_file = self.config.get('log_file', None)

    def _initialize_validators(self) -> List[BaseValidator]:
        """Initialize all configured validators."""
        validators = []

        # File validators
        if self.config.get('file_validation', {}).get('enabled', True):
            file_config = self.config.get('file_validation', {})
            validators.append(FilePathValidator(file_config.get('path_validation', {})))
            validators.append(FileContentValidator(file_config.get('content_validation', {})))

        # Security validators
        if self.config.get('security_validation', {}).get('enabled', True):
            security_config = self.config.get('security_validation', {})
            validators.append(SecurityValidator(security_config.get('secret_scanning', {})))
            validators.append(PermissionValidator(security_config.get('permissions', {})))

        # Pattern validators
        if self.config.get('pattern_validation', {}).get('enabled', True):
            pattern_config = self.config.get('pattern_validation', {})
            validators.append(PatternValidator(pattern_config))

        # Command validators
        if self.config.get('command_validation', {}).get('enabled', True):
            command_config = self.config.get('command_validation', {})
            validators.append(CommandValidator(command_config))

        return validators

    def validate(self, input_data: Dict[str, Any]) -> ValidationResult:
        """Run all validators on the input data."""
        if not self.enabled:
            return ValidationResult(True, "Validation disabled")

        # Create composite validator
        composite = CompositeValidator(self.validators, {'name': 'ValidationOrchestrator'})

        # Run validation
        result = composite.validate(input_data)

        # Log result if configured
        if self.log_file:
            self._log_validation(input_data, result)

        return result

    def _log_validation(self, input_data: Dict[str, Any], result: ValidationResult):
        """Log validation results to file."""
        try:
            import datetime
            log_entry = {
                'timestamp': datetime.datetime.now().isoformat(),
                'tool_name': input_data.get('tool_name', ''),
                'file_path': input_data.get('tool_input', {}).get('file_path', ''),
                'result': result.to_dict()
            }

            # Append to log file
            log_path = Path(self.log_file).expanduser()
            log_path.parent.mkdir(parents=True, exist_ok=True)

            with open(log_path, 'a') as f:
                f.write(json.dumps(log_entry) + '\n')
        except Exception as e:
            sys.stderr.write(f"Failed to log validation: {e}\n")

    def handle_result(self, result: ValidationResult):
        """Handle validation result based on mode."""
        if self.mode == 'silent':
            # Silent mode - log but don't block or show messages
            sys.exit(0)
        elif self.mode == 'warning':
            # Warning mode - show messages but never block
            if not result.is_valid or result.severity == 'warning':
                sys.stderr.write(f"⚠️ Validation: {result.message}\n")
            sys.exit(0)
        else:
            # Blocking mode - block on errors
            handle_validation_result(result, blocking=True)

def main():
    """Main entry point for validation hook."""
    try:
        # Read input from stdin
        input_data = json.load(sys.stdin)

        # Create orchestrator
        orchestrator = ValidationOrchestrator()

        # Run validation
        result = orchestrator.validate(input_data)

        # Handle result
        orchestrator.handle_result(result)

    except json.JSONDecodeError as e:
        sys.stderr.write(f"Invalid JSON input: {e}\n")
        sys.exit(1)
    except Exception as e:
        sys.stderr.write(f"Validation error: {e}\n")
        # Don't block on validation errors
        sys.exit(0)

if __name__ == "__main__":
    main()
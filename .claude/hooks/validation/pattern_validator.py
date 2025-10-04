#!/usr/bin/env python3
"""
Pattern Validator
Validates code against project patterns and conventions
"""
import re
from typing import Dict, Any, Optional, List
from pathlib import Path
from base_validator import BaseValidator, ValidationResult

class PatternValidator(BaseValidator):
    """Validates code against project-specific patterns and conventions."""

    def __init__(self, config: Optional[Dict[str, Any]] = None):
        super().__init__(config)
        self.enforce_naming = self.config.get('enforce_naming', True)
        self.enforce_structure = self.config.get('enforce_structure', True)
        self.custom_patterns = self.config.get('custom_patterns', [])

    @property
    def name(self) -> str:
        return "PatternValidator"

    @property
    def description(self) -> str:
        return "Validates code patterns and conventions"

    def validate(self, data: Dict[str, Any]) -> ValidationResult:
        """Validate code patterns."""
        tool_name = data.get('tool_name', '')
        tool_input = data.get('tool_input', {})

        # Only validate file operations
        if tool_name not in ['Write', 'Edit', 'MultiEdit']:
            return ValidationResult(True, "Not a file operation")

        file_path = tool_input.get('file_path', '')
        content = tool_input.get('content', '') or tool_input.get('new_string', '')

        if not file_path:
            return ValidationResult(True, "No file path to validate")

        # Validate file naming conventions
        if self.enforce_naming:
            naming_result = self._validate_file_naming(file_path)
            if not naming_result:
                return naming_result

        # Validate code patterns if content provided
        if content and self.enforce_structure:
            structure_result = self._validate_code_structure(file_path, content)
            if not structure_result:
                return structure_result

        # Check custom patterns
        for custom in self.custom_patterns:
            pattern = custom.get('pattern')
            message = custom.get('message', 'Custom pattern violation')
            severity = custom.get('severity', 'warning')
            applies_to = custom.get('applies_to', [])

            # Check if pattern applies to this file
            if applies_to:
                file_ext = Path(file_path).suffix
                if file_ext not in applies_to:
                    continue

            if pattern and re.search(pattern, content):
                return ValidationResult(
                    severity != 'error',
                    message,
                    severity
                )

        return ValidationResult(True, "Pattern validation passed")

    def _validate_file_naming(self, file_path: str) -> ValidationResult:
        """Validate file naming conventions."""
        path = Path(file_path)
        file_name = path.stem
        extension = path.suffix

        # Python naming conventions
        if extension == '.py':
            if not re.match(r'^[a-z_][a-z0-9_]*$', file_name):
                if not file_name[0].isupper():  # Allow CamelCase for classes
                    return ValidationResult(
                        True,
                        f"Python file should use snake_case: {file_name}",
                        'warning'
                    )

        # JavaScript/TypeScript naming conventions
        elif extension in ['.js', '.ts', '.jsx', '.tsx']:
            # Components should be PascalCase
            if extension in ['.jsx', '.tsx'] or 'Component' in file_name:
                if not re.match(r'^[A-Z][a-zA-Z0-9]*$', file_name):
                    return ValidationResult(
                        True,
                        f"Component file should use PascalCase: {file_name}",
                        'warning'
                    )
            # Other files should be camelCase or kebab-case
            elif not re.match(r'^[a-z][a-zA-Z0-9]*$|^[a-z]+(-[a-z]+)*$', file_name):
                return ValidationResult(
                    True,
                    f"JavaScript file should use camelCase or kebab-case: {file_name}",
                    'warning'
                )

        # Rust naming conventions
        elif extension == '.rs':
            if not re.match(r'^[a-z_][a-z0-9_]*$', file_name):
                return ValidationResult(
                    True,
                    f"Rust file should use snake_case: {file_name}",
                    'warning'
                )

        return ValidationResult(True, "File naming validation passed")

    def _validate_code_structure(self, file_path: str, content: str) -> ValidationResult:
        """Validate code structure and patterns."""
        path = Path(file_path)
        extension = path.suffix

        # Python patterns
        if extension == '.py':
            return self._validate_python_patterns(content)

        # JavaScript/TypeScript patterns
        elif extension in ['.js', '.ts', '.jsx', '.tsx']:
            return self._validate_javascript_patterns(content)

        # Rust patterns
        elif extension == '.rs':
            return self._validate_rust_patterns(content)

        return ValidationResult(True, "Code structure validation passed")

    def _validate_python_patterns(self, content: str) -> ValidationResult:
        """Validate Python-specific patterns."""
        issues = []

        # Check for proper imports
        lines = content.split('\n')
        import_section_ended = False
        for i, line in enumerate(lines):
            stripped = line.strip()

            # Skip comments and docstrings
            if stripped.startswith('#') or stripped.startswith('"""'):
                continue

            # Check if we're past imports
            if stripped and not stripped.startswith(('import ', 'from ')):
                import_section_ended = True

            # Check for imports after code
            if import_section_ended and stripped.startswith(('import ', 'from ')):
                issues.append(f"Import found after code at line {i+1}")

        # Check for TODO comments without issue references
        todo_pattern = r'#\s*TODO(?![\s:]*[A-Z]+-\d+)'
        if re.search(todo_pattern, content):
            issues.append("TODO comment without issue reference")

        # Check for print statements (should use logging)
        if re.search(r'\bprint\s*\(', content):
            issues.append("Use logging instead of print statements")

        # Check for bare except clauses
        if re.search(r'\bexcept\s*:', content):
            issues.append("Avoid bare except clauses")

        if issues:
            return ValidationResult(
                True,
                "Python pattern issues: " + "; ".join(issues),
                'warning'
            )

        return ValidationResult(True, "Python patterns validated")

    def _validate_javascript_patterns(self, content: str) -> ValidationResult:
        """Validate JavaScript/TypeScript patterns."""
        issues = []

        # Check for console.log (should be removed in production)
        if re.search(r'console\.(log|error|warn|debug)', content):
            issues.append("Remove console statements for production")

        # Check for var usage (prefer const/let)
        if re.search(r'\bvar\s+\w+', content):
            issues.append("Use const/let instead of var")

        # Check for == (prefer ===)
        if re.search(r'[^=!]==[^=]', content):
            issues.append("Use === instead of ==")

        # Check for missing semicolons (if not using a formatter)
        lines = content.split('\n')
        for i, line in enumerate(lines):
            stripped = line.strip()
            if stripped and not stripped.startswith('//'):
                if stripped[-1] not in [';', '{', '}', ','] and not stripped.startswith(('import', 'export')):
                    # Simple heuristic, not perfect
                    if re.match(r'^(const|let|var|return)\s+', stripped):
                        issues.append(f"Possible missing semicolon at line {i+1}")

        if issues:
            return ValidationResult(
                True,
                "JavaScript pattern issues: " + "; ".join(issues[:3]),  # Limit to 3 issues
                'warning'
            )

        return ValidationResult(True, "JavaScript patterns validated")

    def _validate_rust_patterns(self, content: str) -> ValidationResult:
        """Validate Rust-specific patterns."""
        issues = []

        # Check for unwrap() usage (prefer proper error handling)
        if re.search(r'\.unwrap\(\)', content):
            issues.append("Avoid unwrap(), use proper error handling")

        # Check for panic! in non-test code
        if not '#[test]' in content and re.search(r'\bpanic!\s*\(', content):
            issues.append("Avoid panic! in production code")

        # Check for missing documentation
        if re.match(r'^pub\s+(fn|struct|enum|trait)', content, re.MULTILINE):
            if not re.search(r'///|/\*\*', content):
                issues.append("Public items should have documentation")

        # Check for TODO comments
        if re.search(r'//\s*TODO(?![\s:]*[A-Z]+-\d+)', content):
            issues.append("TODO comment without issue reference")

        if issues:
            return ValidationResult(
                True,
                "Rust pattern issues: " + "; ".join(issues),
                'warning'
            )

        return ValidationResult(True, "Rust patterns validated")
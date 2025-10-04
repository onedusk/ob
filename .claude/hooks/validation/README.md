# Validation Hook System

A comprehensive validation framework for Claude Code that ensures safety, security, and code quality through modular validators.

## Overview

The validation system provides multiple layers of protection and quality assurance:
- **File Path Security** - Prevents dangerous file operations
- **Content Security** - Scans for secrets and sensitive data
- **Command Safety** - Validates shell commands before execution
- **Pattern Enforcement** - Ensures code follows project conventions
- **Permission Control** - Enforces access policies

## Components

### Core Framework
- `base_validator.py` - Abstract base classes and utilities
- `validation_orchestrator.py` - Main coordinator that runs all validators
- `validation_config.json` - Configuration for all validators

### Validators

#### 1. File Validators (`file_validator.py`)
- **FilePathValidator** - Validates file paths for security
  - Detects path traversal attempts
  - Blocks protected directories/files
  - Validates symlinks and hidden files

- **FileContentValidator** - Validates file content
  - Enforces size limits
  - Detects binary content
  - Scans for sensitive patterns

#### 2. Security Validators (`security_validator.py`)
- **SecurityValidator** - Comprehensive security checks
  - Secret detection (API keys, passwords, tokens)
  - Dangerous command detection
  - URL validation for web operations

- **PermissionValidator** - Policy enforcement
  - Tool access control
  - Time-based restrictions
  - Approval requirements

#### 3. Pattern Validator (`pattern_validator.py`)
- Enforces naming conventions
- Validates code structure
- Language-specific patterns (Python, JS/TS, Rust)
- Custom pattern matching

#### 4. Command Validator (`command_validator.py`)
- Shell command safety checks
- Dangerous operation blocking
- Common mistake detection
- Alternative command suggestions

## Usage

### Quick Setup

1. **Add to Claude Code settings** (`.claude/settings.json`):
```json
{
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "*",
        "hooks": [
          {
            "type": "command",
            "command": "$CLAUDE_PROJECT_DIR/.claude/hooks/validation/validation_orchestrator.py",
            "timeout": 5000
          }
        ]
      }
    ]
  }
}
```

2. **Configure validators** in `validation_config.json`

3. **Test the system**:
```bash
echo '{"tool_name":"Bash","tool_input":{"command":"rm -rf /"}}' | python3 validation_orchestrator.py
```

## Configuration

### Global Settings
```json
{
  "enabled": true,           // Enable/disable entire system
  "mode": "warning",         // "blocking", "warning", or "silent"
  "log_file": "~/.claude/validation.log"  // Optional logging
}
```

### Modes
- **blocking** - Stops operations on validation failure
- **warning** - Shows warnings but allows operations
- **silent** - Logs issues without user feedback

### Per-Validator Configuration

Each validator can be configured independently:

#### File Validation
```json
"file_validation": {
  "enabled": true,
  "path_validation": {
    "protected_directories": [...],
    "max_path_depth": 20,
    "allow_symlinks": false
  }
}
```

#### Security Validation
```json
"security_validation": {
  "secret_scanning": {
    "scan_for_secrets": true,
    "block_on_secrets": false
  }
}
```

#### Pattern Validation
```json
"pattern_validation": {
  "custom_patterns": [
    {
      "pattern": "console\\.log",
      "message": "Remove console.log",
      "severity": "warning",
      "applies_to": [".js", ".ts"]
    }
  ]
}
```

## Examples

### Blocking Dangerous Commands
```bash
# This will be blocked
echo '{"tool_name":"Bash","tool_input":{"command":"rm -rf /"}}' | python3 validation_orchestrator.py
# Output: BLOCKED: Recursive deletion on root or root-level directory
```

### Detecting Secrets
```bash
# This will warn about exposed secret
echo '{"tool_name":"Write","tool_input":{"file_path":"config.js","content":"const API_KEY=\"sk_live_abc123\""}}' | python3 validation_orchestrator.py
# Output: Warning: Secret detected in content: Stripe Live Secret Key (severity: critical)
```

### Pattern Violations
```python
# This will warn about naming convention
echo '{"tool_name":"Write","tool_input":{"file_path":"MyFile.py","content":"print(\"test\")"}}' | python3 validation_orchestrator.py
# Output: Python file should use snake_case: MyFile
```

## Custom Validators

Create custom validators by extending `BaseValidator`:

```python
from base_validator import BaseValidator, ValidationResult

class MyCustomValidator(BaseValidator):
    @property
    def name(self) -> str:
        return "MyValidator"

    @property
    def description(self) -> str:
        return "Custom validation logic"

    def validate(self, data: Dict[str, Any]) -> ValidationResult:
        # Your validation logic
        if self._is_invalid(data):
            return ValidationResult(False, "Validation failed", "error")
        return ValidationResult(True, "Validation passed")
```

## Testing

### Unit Testing
```bash
# Test individual validators
python3 -c "from file_validator import FilePathValidator;
v = FilePathValidator();
print(v.validate({'tool_input': {'file_path': '../../../etc/passwd'}}))"
```

### Integration Testing
```bash
# Test full orchestration
cat test_cases.json | python3 validation_orchestrator.py
```

### Manual Testing
```bash
# Test specific scenarios
echo '{"tool_name":"Edit","tool_input":{"file_path":".env","content":"SECRET=value"}}' | python3 validation_orchestrator.py
```

## Security Considerations

1. **Default Deny** - Block by default for critical security issues
2. **Defense in Depth** - Multiple validation layers
3. **Fail Safe** - Validation errors don't block operations (unless configured)
4. **Audit Trail** - Optional logging of all validations
5. **No Secret Exposure** - Validators never log actual secret values

## Performance

- Validators run in sequence (fail-fast)
- Regex patterns are pre-compiled
- File operations are minimized
- Average overhead: <50ms per validation

## Troubleshooting

### Validation Not Running
1. Check hook registration: `/hooks`
2. Verify script permissions: `ls -la validation/*.py`
3. Test directly: `echo '{"tool_name":"test"}' | python3 validation_orchestrator.py`

### Too Many False Positives
1. Switch to warning mode: `"mode": "warning"`
2. Adjust sensitivity in config
3. Add exceptions for specific patterns

### Performance Issues
1. Disable unused validators
2. Reduce pattern complexity
3. Use file extension filters

## Best Practices

1. **Start in Warning Mode** - Monitor before blocking
2. **Customize for Your Project** - Add project-specific patterns
3. **Regular Updates** - Keep security patterns current
4. **Test Thoroughly** - Validate doesn't block legitimate operations
5. **Monitor Logs** - Review validation.log for patterns

## Future Enhancements

- [ ] Async validation for better performance
- [ ] Machine learning for pattern detection
- [ ] Integration with external security tools
- [ ] Caching for repeated validations
- [ ] Web UI for configuration management
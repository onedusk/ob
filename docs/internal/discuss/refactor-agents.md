# Parallel Refactoring Agents for Uber Scanner

## Architecture Overview
Each agent works in its own git worktree to avoid conflicts. Agents are specialized for specific refactoring tasks and can run concurrently.

## Agent Definitions

### 1. Module Splitter Agent
**Branch**: `refactor/modularize`
**Priority**: CRITICAL (blocks other agents)
**Task**: Break the monolithic main.rs into proper modules

```yaml
agent: module-splitter
mission: |
  Split src/main.rs into these modules:
  - src/cli.rs - Command definitions and argument parsing
  - src/scanner.rs - Scan functionality  
  - src/replacer.rs - Replace/undo functionality
  - src/patterns.rs - Pattern compilation and management
  - src/config.rs - Config file resolution and loading
  - src/errors.rs - Custom error types
  - src/lib.rs - Public API
  - Keep main.rs as thin entry point only

requirements:
  - Preserve all functionality
  - Fix visibility (pub/private) properly
  - Update imports
  - Ensure cargo build --release works
```

### 2. Error Handler Agent  
**Branch**: `refactor/error-handling`
**Priority**: HIGH
**Depends on**: Module Splitter
**Task**: Implement proper error handling

```yaml
agent: error-handler
mission: |
  Replace Box<dyn Error> with proper error types:
  - Create custom error enum in src/errors.rs
  - Add context to all error paths
  - Use thiserror or anyhow crate
  - Add error recovery in parallel processing
  - Ensure errors bubble up with proper context

requirements:
  - Every ? operator should have context
  - No silent failures in rayon iterations
  - User-friendly error messages
```

### 3. Test Writer Agent
**Branch**: `refactor/add-tests`  
**Priority**: HIGH
**Depends on**: Module Splitter
**Task**: Add comprehensive test coverage

```yaml
agent: test-writer
mission: |
  Create test modules for each component:
  - tests/scanner_tests.rs - Pattern matching tests
  - tests/replacer_tests.rs - Replacement logic tests
  - tests/config_tests.rs - Config loading tests
  - tests/integration_tests.rs - End-to-end tests
  - Add unit tests in each module
  - Create test fixtures in tests/fixtures/

requirements:
  - Test edge cases (empty files, binary files, symlinks)
  - Test all regex patterns
  - Test backup/restore functionality
  - Test parallel processing
  - Achieve 80%+ code coverage
```

### 4. Pattern Optimizer Agent
**Branch**: `refactor/optimize-patterns`
**Priority**: MEDIUM
**Depends on**: Module Splitter
**Task**: Optimize regex compilation and caching

```yaml
agent: pattern-optimizer
mission: |
  Optimize pattern handling:
  - Implement lazy_static or once_cell for pattern caching
  - Pre-compile all patterns at startup
  - Add pattern validation before use
  - Create RegexCache struct
  - Benchmark before/after performance

requirements:
  - Patterns compile once, reuse everywhere
  - Invalid patterns fail fast with clear errors
  - Document regex performance characteristics
```

### 5. Preset Migrator Agent
**Branch**: `refactor/yaml-presets`
**Priority**: MEDIUM
**Task**: Move hardcoded presets to YAML

```yaml
agent: preset-migrator
mission: |
  Extract hardcoded presets to YAML:
  - Create presets/ directory
  - Move each preset to presets/<name>.yaml
  - Implement preset loader in config module
  - Remove hardcoded get_preset_config function
  - Add preset validation

requirements:
  - All existing presets work identically
  - Users can add custom presets
  - Presets are validated at load time
```

### 6. Benchmark Agent
**Branch**: `refactor/add-benchmarks`
**Priority**: LOW
**Depends on**: Module Splitter, Pattern Optimizer
**Task**: Add performance benchmarks

```yaml
agent: benchmark-creator
mission: |
  Create benchmarks to validate performance:
  - Add criterion to dev-dependencies
  - Create benches/scan_bench.rs
  - Create benches/replace_bench.rs  
  - Benchmark against ripgrep for scanning
  - Generate performance report
  - Add benchmark CI job

requirements:
  - Measure throughput (MB/s)
  - Compare single vs multi-threaded
  - Test with various file sizes
  - Document performance characteristics
```

## Orchestration Script

```bash
#!/bin/bash
# orchestrate-refactor.sh

set -e

REPO_ROOT=$(git rev-parse --show-toplevel)
AGENTS_DIR="$REPO_ROOT/.refactor-agents"

# Create worktrees for each agent
create_worktree() {
    local branch=$1
    local dir=$2
    
    if [ ! -d "$dir" ]; then
        git worktree add -b "$branch" "$dir" HEAD
    fi
}

# Phase 1: Critical agent (blocks others)
echo "Phase 1: Module Splitter"
create_worktree "refactor/modularize" "$AGENTS_DIR/module-splitter"

# Wait for module splitter to complete
echo "Run module-splitter agent in $AGENTS_DIR/module-splitter"
echo "When complete, commit and push, then press Enter"
read -r

# Phase 2: Parallel agents (depend on module splitter)
echo "Phase 2: Parallel Refactoring"

create_worktree "refactor/error-handling" "$AGENTS_DIR/error-handler"
create_worktree "refactor/add-tests" "$AGENTS_DIR/test-writer"  
create_worktree "refactor/optimize-patterns" "$AGENTS_DIR/pattern-optimizer"
create_worktree "refactor/yaml-presets" "$AGENTS_DIR/preset-migrator"

echo "Run these agents in parallel:"
echo "  - error-handler in $AGENTS_DIR/error-handler"
echo "  - test-writer in $AGENTS_DIR/test-writer"
echo "  - pattern-optimizer in $AGENTS_DIR/pattern-optimizer"
echo "  - preset-migrator in $AGENTS_DIR/preset-migrator"
echo "When all complete, press Enter"
read -r

# Phase 3: Final agent (depends on others)
echo "Phase 3: Benchmarking"
create_worktree "refactor/add-benchmarks" "$AGENTS_DIR/benchmark-creator"

echo "Run benchmark-creator in $AGENTS_DIR/benchmark-creator"
echo "When complete, press Enter"
read -r

# Merge all branches
echo "Merging all refactor branches..."
git checkout main
for branch in refactor/modularize refactor/error-handling refactor/add-tests \
              refactor/optimize-patterns refactor/yaml-presets refactor/add-benchmarks; do
    git merge --no-ff "$branch" -m "Merge $branch"
done

echo "Refactoring complete!"
```

## Agent Prompts Directory Structure

```
.claude/agents/
 module-splitter.md
 error-handler.md
 test-writer.md
 pattern-optimizer.md
 preset-migrator.md
 benchmark-creator.md
```

## Execution Instructions

1. **Setup**: Run `./orchestrate-refactor.sh` to create worktrees
2. **Phase 1**: Run module-splitter agent first (critical path)
3. **Phase 2**: Run 4 agents in parallel in their worktrees
4. **Phase 3**: Run benchmark agent after others complete
5. **Merge**: Script automatically merges all branches

## Expected Outcomes

- **Code Structure**: Proper module separation, 100-200 lines per file max
- **Test Coverage**: 80%+ with comprehensive edge cases
- **Performance**: Validated benchmarks, optimized regex caching
- **Error Handling**: Contextual errors, no silent failures
- **Maintainability**: YAML-based configuration, clear separation of concerns

## Success Metrics

- All tests pass: `cargo test`
- Benchmarks run: `cargo bench`
- No performance regression vs current version
- Clean clippy: `cargo clippy -- -D warnings`
- Documented API: `cargo doc --open`
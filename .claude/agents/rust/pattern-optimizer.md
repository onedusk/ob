# Pattern Optimizer Agent

## Mission
Optimize regex compilation and caching to eliminate redundant compilation and improve performance.

## Prerequisites
- Module Splitter agent must complete first
- Work from the `refactor/modularize` branch

## Current Problems

1. **Patterns compiled every execution** (main.rs:297-309)
   - Regex compilation happens in replace_command every time
   - No caching between invocations
   
2. **No pattern validation**
   - Invalid patterns fail at runtime
   - No early validation during config load

3. **Inefficient pattern matching**
   - Could use regex::RegexSet for multiple patterns
   - No optimization for literal vs regex patterns

## Implementation Plan

### 1. Add dependencies
```toml
# Cargo.toml
[dependencies]
once_cell = "1.19"  # For lazy static patterns
lru = "0.12"        # LRU cache for dynamic patterns
regex = { version = "1.9", features = ["perf"] }  # Enable performance features
```

### 2. Create Pattern Cache (src/patterns/cache.rs)

```rust
use once_cell::sync::Lazy;
use lru::LruCache;
use regex::{Regex, RegexSet, RegexSetBuilder};
use std::sync::Mutex;
use std::num::NonZeroUsize;

/// Global pattern cache for frequently used patterns
static PATTERN_CACHE: Lazy<Mutex<LruCache<String, Regex>>> = Lazy::new(|| {
    Mutex::new(LruCache::new(NonZeroUsize::new(128).unwrap()))
});

/// Preset patterns compiled at startup
static PRESET_PATTERNS: Lazy<PresetPatterns> = Lazy::new(|| {
    PresetPatterns::compile_all()
});

pub struct PatternCache {
    local_cache: LruCache<String, Regex>,
    stats: CacheStats,
}

#[derive(Debug, Default)]
pub struct CacheStats {
    pub hits: u64,
    pub misses: u64,
    pub compilation_time_ms: u64,
}

impl PatternCache {
    pub fn new(capacity: usize) -> Self {
        Self {
            local_cache: LruCache::new(NonZeroUsize::new(capacity).unwrap()),
            stats: CacheStats::default(),
        }
    }
    
    /// Get or compile a pattern with caching
    pub fn get_or_compile(&mut self, pattern: &str) -> Result<&Regex> {
        use std::time::Instant;
        
        // Check if it's a literal string (no regex special chars)
        if self.is_literal_pattern(pattern) {
            return self.compile_literal(pattern);
        }
        
        // Check local cache first
        if let Some(regex) = self.local_cache.get(pattern) {
            self.stats.hits += 1;
            return Ok(regex);
        }
        
        // Check global cache
        if let Ok(global) = PATTERN_CACHE.lock() {
            if let Some(regex) = global.peek(pattern) {
                self.stats.hits += 1;
                let cloned = regex.clone();
                self.local_cache.put(pattern.to_string(), cloned);
                return self.local_cache.get(pattern).ok_or("Cache error");
            }
        }
        
        // Compile new pattern
        self.stats.misses += 1;
        let start = Instant::now();
        
        let regex = Regex::new(pattern)
            .map_err(|e| PatternError::CompilationFailed {
                pattern: pattern.to_string(),
                error: e.to_string(),
            })?;
        
        self.stats.compilation_time_ms += start.elapsed().as_millis() as u64;
        
        // Add to both caches
        if let Ok(mut global) = PATTERN_CACHE.lock() {
            global.put(pattern.to_string(), regex.clone());
        }
        
        self.local_cache.put(pattern.to_string(), regex);
        self.local_cache.get(pattern).ok_or("Cache error")
    }
    
    /// Check if pattern is a literal (no regex special chars)
    fn is_literal_pattern(&self, pattern: &str) -> bool {
        !pattern.chars().any(|c| matches!(c, 
            '.' | '^' | '$' | '*' | '+' | '?' | '(' | ')' | 
            '[' | ']' | '{' | '}' | '|' | '\\'
        ))
    }
    
    /// Optimize literal patterns
    fn compile_literal(&mut self, literal: &str) -> Result<&Regex> {
        let escaped = regex::escape(literal);
        self.get_or_compile(&escaped)
    }
    
    pub fn stats(&self) -> &CacheStats {
        &self.stats
    }
}
```

### 3. Pattern Set Optimization (src/patterns/set.rs)

```rust
use regex::{RegexSet, RegexSetBuilder};

/// Optimized pattern set for matching multiple patterns at once
pub struct PatternSet {
    patterns: Vec<String>,
    regex_set: RegexSet,
    individual_regexes: Vec<Regex>,
}

impl PatternSet {
    pub fn new(patterns: Vec<String>) -> Result<Self> {
        // Build regex set for fast multi-pattern matching
        let regex_set = RegexSetBuilder::new(&patterns)
            .size_limit(10 * 1024 * 1024) // 10MB limit
            .dfa_size_limit(10 * 1024 * 1024)
            .build()
            .map_err(|e| PatternError::SetCompilationFailed(e))?;
        
        // Compile individual regexes for extraction
        let individual_regexes = patterns
            .iter()
            .map(|p| Regex::new(p))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| PatternError::CompilationFailed {
                pattern: String::new(),
                error: e.to_string(),
            })?;
        
        Ok(Self {
            patterns,
            regex_set,
            individual_regexes,
        })
    }
    
    /// Check which patterns match (returns indices)
    pub fn matches(&self, text: &str) -> Vec<usize> {
        self.regex_set.matches(text).into_iter().collect()
    }
    
    /// Check if any pattern matches
    pub fn is_match(&self, text: &str) -> bool {
        self.regex_set.is_match(text)
    }
    
    /// Get all matches with their positions
    pub fn find_all<'t>(&self, text: &'t str) -> Vec<Match<'t>> {
        let matching_indices = self.matches(text);
        let mut all_matches = Vec::new();
        
        for idx in matching_indices {
            let regex = &self.individual_regexes[idx];
            for mat in regex.find_iter(text) {
                all_matches.push(Match {
                    pattern_index: idx,
                    pattern_name: &self.patterns[idx],
                    start: mat.start(),
                    end: mat.end(),
                    text: mat.as_str(),
                });
            }
        }
        
        all_matches.sort_by_key(|m| m.start);
        all_matches
    }
}

pub struct Match<'t> {
    pub pattern_index: usize,
    pub pattern_name: &'t str,
    pub start: usize,
    pub end: usize,
    pub text: &'t str,
}
```

### 4. Preset Pattern Compilation (src/patterns/presets.rs)

```rust
/// Precompiled preset patterns loaded at startup
pub struct PresetPatterns {
    clean_debug: PatternSet,
    remove_todos: PatternSet,
    trim_whitespace: Regex,
    // Add more presets
}

impl PresetPatterns {
    fn compile_all() -> Self {
        // These compile once at program start
        Self {
            clean_debug: PatternSet::new(vec![
                r"console\.\w+\([^)]*\)".into(),
                r"debugger\s*;?".into(),
                r"logger\.debug\([^)]*\)".into(),
            ]).expect("Failed to compile debug patterns"),
            
            remove_todos: PatternSet::new(vec![
                r".*//\s*TODO:?.*".into(),
                r".*//\s*FIXME:?.*".into(),
                r".*#\s*TODO:?.*".into(),
            ]).expect("Failed to compile TODO patterns"),
            
            trim_whitespace: Regex::new(r"[ \t]+$")
                .expect("Failed to compile whitespace pattern"),
        }
    }
    
    pub fn get(&self, preset: &Preset) -> &PatternSet {
        match preset {
            Preset::CleanDebug => &self.clean_debug,
            Preset::RemoveTodos => &self.remove_todos,
            // ...
        }
    }
}
```

### 5. Pattern Validation (src/patterns/validator.rs)

```rust
use regex::Regex;

pub struct PatternValidator;

impl PatternValidator {
    /// Validate pattern at config load time
    pub fn validate(pattern: &str) -> Result<ValidationReport> {
        let mut report = ValidationReport::default();
        
        // Try to compile
        match Regex::new(pattern) {
            Ok(regex) => {
                report.valid = true;
                
                // Check for performance issues
                if pattern.starts_with(".*") {
                    report.warnings.push(
                        "Pattern starts with '.*' which can be slow".into()
                    );
                }
                
                if pattern.contains("(.*)") {
                    report.warnings.push(
                        "Greedy capture group '(.*)' can cause backtracking".into()
                    );
                }
                
                // Check complexity
                let complexity = self.estimate_complexity(pattern);
                if complexity > 100 {
                    report.warnings.push(format!(
                        "Pattern complexity score: {} (>100 may be slow)", 
                        complexity
                    ));
                }
            }
            Err(e) => {
                report.valid = false;
                report.error = Some(format!("Invalid regex: {}", e));
                
                // Provide helpful hints
                if pattern.contains("(") && !pattern.contains(")") {
                    report.hints.push("Unclosed parenthesis - add ')' or escape '('".into());
                }
                
                if pattern.contains("[") && !pattern.contains("]") {
                    report.hints.push("Unclosed bracket - add ']' or escape '['".into());
                }
            }
        }
        
        Ok(report)
    }
    
    fn estimate_complexity(&self, pattern: &str) -> u32 {
        let mut score = pattern.len() as u32;
        
        // Penalize expensive operations
        score += pattern.matches(".*").count() as u32 * 10;
        score += pattern.matches("\\w*").count() as u32 * 5;
        score += pattern.matches("|").count() as u32 * 3;
        score += pattern.matches("(?i)").count() as u32 * 2;
        
        score
    }
}

#[derive(Default)]
pub struct ValidationReport {
    pub valid: bool,
    pub error: Option<String>,
    pub warnings: Vec<String>,
    pub hints: Vec<String>,
}
```

### 6. Benchmark Comparisons

```rust
#[cfg(test)]
mod bench {
    use super::*;
    use criterion::{black_box, Criterion};
    
    fn bench_cached_vs_uncached(c: &mut Criterion) {
        let patterns = vec![
            "console\\.log",
            "TODO",
            "FIXME",
            // ... 100 patterns
        ];
        
        c.bench_function("uncached compilation", |b| {
            b.iter(|| {
                for p in &patterns {
                    let _ = Regex::new(black_box(p));
                }
            });
        });
        
        c.bench_function("cached compilation", |b| {
            let mut cache = PatternCache::new(128);
            b.iter(|| {
                for p in &patterns {
                    let _ = cache.get_or_compile(black_box(p));
                }
            });
        });
    }
    
    fn bench_pattern_set(c: &mut Criterion) {
        let text = "Large text with console.log() and TODO items...";
        let patterns = vec!["console", "TODO", "FIXME"];
        
        c.bench_function("individual regex matching", |b| {
            let regexes: Vec<_> = patterns.iter()
                .map(|p| Regex::new(p).unwrap())
                .collect();
            
            b.iter(|| {
                for re in &regexes {
                    let _ = re.find_iter(black_box(text)).count();
                }
            });
        });
        
        c.bench_function("regex set matching", |b| {
            let set = PatternSet::new(patterns.clone()).unwrap();
            
            b.iter(|| {
                let _ = set.find_all(black_box(text));
            });
        });
    }
}
```

## Performance Targets

- Pattern compilation: < 1ms for typical patterns
- Cache hit rate: > 90% after warmup
- Memory usage: < 10MB for pattern cache
- RegexSet performance: 2-5x faster than individual matching

## Integration Points

Update scanner.rs:
```rust
pub struct Scanner {
    pattern_cache: PatternCache,
    pattern_set: Option<PatternSet>,
}

impl Scanner {
    pub fn scan_file(&mut self, path: &Path) -> Result<Vec<Match>> {
        // Use PatternSet for multi-pattern matching
        if let Some(ref set) = self.pattern_set {
            // Fast path: check if any patterns match
            if !set.is_match(&content) {
                return Ok(vec![]);
            }
            // Extract all matches
            return Ok(set.find_all(&content));
        }
        
        // Fallback to individual patterns
        // ...
    }
}
```

## Success Criteria

- Zero regex compilation in hot paths
- Pattern validation at config load time
- Cache statistics show > 90% hit rate
- Benchmarks show 2-5x performance improvement
- Memory usage remains under 10MB
- Invalid patterns fail fast with helpful errors
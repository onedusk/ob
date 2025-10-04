use crate::cli::Preset;
use crate::config::{Block, ReplaceConfig};
use crate::errors::Result;
use regex::Regex;
use std::collections::HashMap;

/// Manages the compilation and caching of regex patterns.
///
/// This struct is used to avoid compiling the same regex multiple times, which can be
/// a performance bottleneck. It maintains an internal cache of compiled `Regex` objects.
pub struct PatternManager {
    cache: HashMap<String, Regex>,
}

impl Default for PatternManager {
    fn default() -> Self {
        Self::new()
    }
}

impl PatternManager {
    /// Creates a new, empty `PatternManager`.
    pub fn new() -> Self {
        Self {
            cache: HashMap::new(),
        }
    }

    /// Compiles a regex pattern string, using the cache if available.
    ///
    /// If the pattern has already been compiled, it returns a reference to the cached
    /// `Regex` object. Otherwise, it compiles the pattern, stores it in the cache,
    /// and then returns the reference.
    pub fn compile(&mut self, pattern: &str) -> Result<&Regex> {
        if !self.cache.contains_key(pattern) {
            let regex = Regex::new(pattern)?;
            self.cache.insert(pattern.to_string(), regex);
        }
        Ok(self.cache.get(pattern).unwrap())
    }

    /// Loads a pre-defined `ReplaceConfig` for a given `Preset`.
    ///
    /// Presets are convenient, built-in configurations for common code cleanup tasks.
    pub fn load_preset(preset: &Preset) -> ReplaceConfig {
        match preset {
            Preset::RemoveCopyright => ReplaceConfig {
                patterns: vec![],
                replacements: vec![],
                blocks: vec![
                    // Common copyright patterns
                    Block {
                        start: "# Copyright (c)".to_string(),
                        end: "# DEALINGS IN THE SOFTWARE.".to_string(),
                    },
                    Block {
                        start: "// Copyright (c)".to_string(),
                        end: "// DEALINGS IN THE SOFTWARE.".to_string(),
                    },
                    Block {
                        start: "/* Copyright (c)".to_string(),
                        end: "*/".to_string(),
                    },
                    Block {
                        start: "/**\n * Copyright (c)".to_string(),
                        end: " */".to_string(),
                    },
                    // MIT License blocks
                    Block {
                        start: "# MIT License".to_string(),
                        end: "# SOFTWARE.".to_string(),
                    },
                    Block {
                        start: "// MIT License".to_string(),
                        end: "// SOFTWARE.".to_string(),
                    },
                    // Apache License blocks
                    Block {
                        start: "# Licensed under the Apache License".to_string(),
                        end: "# limitations under the License.".to_string(),
                    },
                    Block {
                        start: "// Licensed under the Apache License".to_string(),
                        end: "// limitations under the License.".to_string(),
                    },
                ],
                extensions: None,
                exclude: None,
            },

            Preset::CleanDebug => ReplaceConfig {
                patterns: vec![
                    "console\\.log\\(.*\\)".to_string(),
                    "console\\.debug\\(.*\\)".to_string(),
                    "console\\.trace\\(.*\\)".to_string(),
                    "debugger;?".to_string(),
                    "print\\(.*\\)\\s*#\\s*DEBUG".to_string(),
                    "print\\(.*\\)\\s*#\\s*debug".to_string(),
                    "logger\\.debug\\(.*\\)".to_string(),
                    "Debug\\.Print\\(.*\\)".to_string(),
                    "System\\.out\\.println\\(.*\\);?\\s*//\\s*DEBUG".to_string(),
                ],
                replacements: vec![
                    Some("// console.log()".to_string()),
                    Some("// console.debug()".to_string()),
                    Some("// console.trace()".to_string()),
                    Some("// debugger;".to_string()),
                    Some("# print() # DEBUG".to_string()),
                    Some("# print() # debug".to_string()),
                    Some("// logger.debug()".to_string()),
                    Some("// Debug.Print()".to_string()),
                    Some("// System.out.println(); // DEBUG".to_string()),
                ],
                blocks: vec![
                    Block {
                        start: "// DEBUG START".to_string(),
                        end: "// DEBUG END".to_string(),
                    },
                    Block {
                        start: "/* DEBUG START */".to_string(),
                        end: "/* DEBUG END */".to_string(),
                    },
                    Block {
                        start: "# DEBUG START".to_string(),
                        end: "# DEBUG END".to_string(),
                    },
                ],
                extensions: None,
                exclude: None,
            },

            Preset::RemoveTodos => ReplaceConfig {
                patterns: vec![
                    ".*//\\s*TODO:?.*".to_string(),
                    ".*//\\s*FIXME:?.*".to_string(),
                    ".*//\\s*HACK:?.*".to_string(),
                    ".*//\\s*XXX:?.*".to_string(),
                    ".*#\\s*TODO:?.*".to_string(),
                    ".*#\\s*FIXME:?.*".to_string(),
                    ".*#\\s*HACK:?.*".to_string(),
                    ".*#\\s*XXX:?.*".to_string(),
                ],
                replacements: vec![
                    Some("".to_string()),
                    Some("".to_string()),
                    Some("".to_string()),
                    Some("".to_string()),
                    Some("".to_string()),
                    Some("".to_string()),
                    Some("".to_string()),
                    Some("".to_string()),
                ],
                blocks: vec![],
                extensions: None,
                exclude: None,
            },

            Preset::TrimWhitespace => ReplaceConfig {
                patterns: vec![
                    "[ \\t]+$".to_string(), // Trailing whitespace
                ],
                replacements: vec![Some("".to_string())],
                blocks: vec![],
                extensions: None,
                exclude: None,
            },

            Preset::RemoveEmptyComments => ReplaceConfig {
                patterns: vec![
                    "^\\s*//\\s*$".to_string(),           // Empty // comments
                    "^\\s*#\\s*$".to_string(),            // Empty # comments
                    "^\\s*/\\*\\s*\\*/\\s*$".to_string(), // Empty /* */ comments
                ],
                replacements: vec![
                    Some("".to_string()),
                    Some("".to_string()),
                    Some("".to_string()),
                ],
                // TODO: The block handling for multi-line empty comments needs a more robust implementation.
                // An empty `end` pattern is not a reliable way to define a block. A better approach
                // would be to have a dedicated pattern that matches empty multi-line comment blocks,
                // like `/\*\s*\*/`.
                blocks: vec![Block {
                    start: "/*\n */".to_string(),
                    end: "".to_string(), // This won't work, need to handle differently
                }],
                extensions: None,
                exclude: None,
            },

            Preset::TabsToSpaces => ReplaceConfig {
                patterns: vec!["\\t".to_string()],
                replacements: vec![
                    Some("    ".to_string()), // 4 spaces
                ],
                blocks: vec![],
                extensions: None,
                exclude: None,
            },

            Preset::SpacesToTabs => ReplaceConfig {
                patterns: vec![
                    "    ".to_string(), // 4 spaces
                ],
                replacements: vec![Some("\t".to_string())],
                blocks: vec![],
                extensions: None,
                exclude: None,
            },
        }
    }
}

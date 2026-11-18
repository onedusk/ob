use crate::cli::Preset;
use crate::config::{Block, ReplaceConfig};

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
                // Only a debug call that is the whole statement on its own
                // line is touched: the call must start the line (after the
                // indent, kept as ${1}), its arguments hold no `;` and stay
                // on the line, and nothing but an optional `;` and
                // whitespace may follow (kept as ${2}). (?mR) as in the
                // presets below: ^/$ match at every LF or CRLF line
                // boundary. [ \t] and [^;\r\n] keep a match on one line.
                "(?mR)^([ \\t]*)console\\.log\\([^;\\r\\n]*\\)([ \\t]*;?[ \\t]*)$".to_string(),
                "(?mR)^([ \\t]*)console\\.debug\\([^;\\r\\n]*\\)([ \\t]*;?[ \\t]*)$".to_string(),
                "(?mR)^([ \\t]*)console\\.trace\\([^;\\r\\n]*\\)([ \\t]*;?[ \\t]*)$".to_string(),
                "(?mR)^([ \\t]*)debugger;?([ \\t]*)$".to_string(),
                // The DEBUG marker is a comment, so whatever follows it on
                // the line is comment text and stays.
                "(?mR)^([ \\t]*)print\\([^;\\r\\n]*\\)[ \\t]*#[ \\t]*DEBUG".to_string(),
                "(?mR)^([ \\t]*)print\\([^;\\r\\n]*\\)[ \\t]*#[ \\t]*debug".to_string(),
                "(?mR)^([ \\t]*)System\\.out\\.println\\([^;\\r\\n]*\\);?[ \\t]*//[ \\t]*DEBUG"
                    .to_string(),
            ],
            replacements: vec![
                Some("${1}// console.log()${2}".to_string()),
                Some("${1}// console.debug()${2}".to_string()),
                Some("${1}// console.trace()${2}".to_string()),
                Some("${1}// debugger;${2}".to_string()),
                Some("${1}# print() # DEBUG".to_string()),
                Some("${1}# print() # debug".to_string()),
                Some("${1}// System.out.println(); // DEBUG".to_string()),
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
                // Only the comment goes, never code. (?mR) as in the presets
                // below: ^/$ match at every LF or CRLF line boundary and .
                // stops before \r. [ \t] instead of \s keeps a match on one
                // line. Order matters: comment-only lines must go first.
                //
                // A line holding only the comment is removed whole, with its
                // line break.
                "(?mR)^[ \\t]*(?://|#)[ \\t]*(?:TODO|FIXME|HACK|XXX).*(?:\\r?\\n)?".to_string(),
                // A trailing comment is cut with the whitespace before it.
                // Requiring that whitespace leaves "http://XXX.example.com",
                // href="#TODO" and "## TODO" alone.
                "(?mR)[ \\t]+(?://|#)[ \\t]*(?:TODO|FIXME|HACK|XXX).*$".to_string(),
            ],
            replacements: vec![Some("".to_string()), Some("".to_string())],
            blocks: vec![],
            extensions: None,
            exclude: None,
        },

        Preset::TrimWhitespace => ReplaceConfig {
            patterns: vec![
                // (?m) makes $ match at every line end; patterns are applied
                // to the whole file content, not line-by-line. R treats \r\n
                // as a line boundary so CRLF lines match too: $ sits before
                // the \r, which stays in place.
                "(?mR)[ \\t]+$".to_string(), // Trailing whitespace
            ],
            replacements: vec![Some("".to_string())],
            blocks: vec![],
            extensions: None,
            exclude: None,
        },

        Preset::RemoveEmptyComments => ReplaceConfig {
            patterns: vec![
                // (?m) makes ^/$ match at every line boundary; patterns are
                // applied to the whole file content, not line-by-line. R
                // treats \r\n as a line boundary so CRLF lines match too:
                // $ sits before the \r, which stays in place.
                "(?mR)^[ \\t]*//[ \\t]*$".to_string(), // Empty // comments
                "(?mR)^[ \\t]*#[ \\t]*$".to_string(),  // Empty # comments
                "(?mR)^[ \\t]*/\\*[ \\t]*\\*/[ \\t]*$".to_string(), // Empty /* */ comments
                // Empty block comments spanning lines ("/*\n */"): patterns
                // run against whole-file content, and \s matches newlines.
                "/\\*\\s*\\*/".to_string(),
            ],
            replacements: vec![
                Some("".to_string()),
                Some("".to_string()),
                Some("".to_string()),
                Some("".to_string()),
            ],
            blocks: vec![],
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

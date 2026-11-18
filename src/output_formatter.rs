use crate::errors::Result;
use crate::scanner::Match;
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::borrow::Cow;
use std::io::Write;
use std::path::{Component, Path, PathBuf};

/// Defines the possible output formats for scan results.
#[derive(Debug, Clone, clap::ValueEnum)]
pub enum OutputFormat {
    /// A simple, human-readable text format.
    Text,
    /// JSON format, suitable for machine processing.
    Json,
    /// Comma-Separated Values format.
    Csv,
    /// Static Analysis Results Interchange Format, for integration with security tools.
    Sarif,
    /// A rich HTML report.
    Html,
}

/// Handles the formatting of scan results into various output formats.
pub struct OutputFormatter {
    format: OutputFormat,
    include_summary: bool,
    tool_name: String,
    tool_version: String,
}

impl OutputFormatter {
    /// Creates a new `OutputFormatter`.
    ///
    /// # Arguments
    ///
    /// * `format` - The `OutputFormat` to use.
    /// * `include_summary` - Whether to include a summary in the output (currently
    ///   only supported for the `Text` format).
    pub fn new(format: OutputFormat, include_summary: bool) -> Self {
        Self {
            format,
            include_summary,
            tool_name: "oober".to_string(),
            tool_version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }

    /// Writes the formatted scan results to a given writer.
    ///
    /// # Arguments
    ///
    /// * `writer` - The `Write` target (e.g., a file or `stdout`).
    /// * `matches` - The `Match` objects to format.
    pub fn write_output<W: Write>(&self, writer: &mut W, matches: &[Match]) -> Result<()> {
        let output = match self.format {
            OutputFormat::Text => self.format_text(matches)?,
            OutputFormat::Json => self.format_json(matches)?,
            OutputFormat::Csv => self.format_csv(matches)?,
            OutputFormat::Sarif => self.format_sarif(matches)?,
            OutputFormat::Html => self.format_html(matches)?,
        };

        writer.write_all(output.as_bytes())?;

        if self.include_summary && matches!(self.format, OutputFormat::Text) {
            let summary = self.format_summary(matches)?;
            writer.write_all(summary.as_bytes())?;
        }

        Ok(())
    }

    /// Formats matches into a simple, human-readable text format.
    fn format_text(&self, matches: &[Match]) -> Result<String> {
        let mut output = String::new();

        for m in matches {
            output.push_str(&format!(
                "[{}] {}:{}: {}\n",
                m.pattern_name,
                m.file_path.display(),
                m.line_number,
                m.line_content
            ));
        }

        Ok(output)
    }

    /// Formats matches into a structured JSON format.
    fn format_json(&self, matches: &[Match]) -> Result<String> {
        #[derive(Serialize)]
        struct JsonOutput {
            tool: ToolInfo,
            scan_time: DateTime<Utc>,
            total_matches: usize,
            matches: Vec<JsonMatch>,
        }

        #[derive(Serialize)]
        struct ToolInfo {
            name: String,
            version: String,
        }

        #[derive(Serialize)]
        struct JsonMatch {
            pattern: String,
            file: String,
            line: usize,
            content: String,
            severity: String,
        }

        let json_matches: Vec<JsonMatch> = matches
            .iter()
            .map(|m| JsonMatch {
                pattern: m.pattern_name.clone(),
                file: m.file_path.display().to_string(),
                line: m.line_number,
                content: m.line_content.trim().to_string(),
                severity: self.get_severity(&m.pattern_name),
            })
            .collect();

        let output = JsonOutput {
            tool: ToolInfo {
                name: self.tool_name.clone(),
                version: self.tool_version.clone(),
            },
            scan_time: Utc::now(),
            total_matches: matches.len(),
            matches: json_matches,
        };

        Ok(serde_json::to_string_pretty(&output)?)
    }

    /// Formats matches into a CSV table.
    fn format_csv(&self, matches: &[Match]) -> Result<String> {
        use csv::Writer;

        let mut wtr = Writer::from_writer(vec![]);

        // Write header
        wtr.write_record(["Pattern", "File", "Line", "Content", "Severity"])?;

        // Write records. Every string field is guarded, not just the matched
        // content: a scanned file or pattern named "=HYPERLINK(...)" injects
        // through its column just as well.
        for m in matches {
            let file = m.file_path.display().to_string();
            let line = m.line_number.to_string();
            let severity = self.get_severity(&m.pattern_name);
            wtr.write_record([
                &*guard_csv_formula(&m.pattern_name),
                &*guard_csv_formula(&file),
                line.as_str(),
                &*guard_csv_formula(m.line_content.trim()),
                &*guard_csv_formula(&severity),
            ])?;
        }

        let data = wtr
            .into_inner()
            .map_err(|e| format!("CSV writer error: {}", e))?;
        Ok(String::from_utf8(data).unwrap_or_default())
    }

    /// Formats matches into the SARIF standard for static analysis results.
    fn format_sarif(&self, matches: &[Match]) -> Result<String> {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct SarifOutput {
            #[serde(rename = "$schema")]
            schema: String,
            version: String,
            runs: Vec<Run>,
        }

        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Run {
            tool: Tool,
            results: Vec<SarifResult>,
        }

        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Tool {
            driver: Driver,
        }

        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Driver {
            name: String,
            version: String,
            rules: Vec<Rule>,
        }

        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Rule {
            id: String,
            name: String,
            short_description: Description,
            default_configuration: Configuration,
        }

        #[derive(Serialize)]
        struct Description {
            text: String,
        }

        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Configuration {
            level: String,
        }

        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct SarifResult {
            rule_id: String,
            level: String,
            message: Message,
            locations: Vec<Location>,
        }

        #[derive(Serialize)]
        struct Message {
            text: String,
        }

        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Location {
            physical_location: PhysicalLocation,
        }

        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct PhysicalLocation {
            artifact_location: ArtifactLocation,
            region: Region,
        }

        #[derive(Serialize)]
        struct ArtifactLocation {
            uri: String,
        }

        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Region {
            start_line: usize,
            snippet: Snippet,
        }

        #[derive(Serialize)]
        struct Snippet {
            text: String,
        }

        // Collect unique patterns for rules
        let mut unique_patterns: Vec<String> =
            matches.iter().map(|m| m.pattern_name.clone()).collect();
        unique_patterns.sort();
        unique_patterns.dedup();

        let rules: Vec<Rule> = unique_patterns
            .iter()
            .map(|pattern| Rule {
                id: pattern.clone(),
                name: pattern.clone(),
                short_description: Description {
                    text: format!("Pattern: {}", pattern),
                },
                default_configuration: Configuration {
                    level: self.get_sarif_level(pattern),
                },
            })
            .collect();

        let results: Vec<SarifResult> = matches
            .iter()
            .map(|m| SarifResult {
                rule_id: m.pattern_name.clone(),
                level: self.get_sarif_level(&m.pattern_name),
                message: Message {
                    text: format!(
                        "Found pattern '{}' at line {}",
                        m.pattern_name, m.line_number
                    ),
                },
                locations: vec![Location {
                    physical_location: PhysicalLocation {
                        artifact_location: ArtifactLocation {
                            uri: sarif_uri(&m.file_path),
                        },
                        region: Region {
                            start_line: m.line_number,
                            snippet: Snippet {
                                text: m.line_content.trim().to_string(),
                            },
                        },
                    },
                }],
            })
            .collect();

        let output = SarifOutput {
            schema: "https://json.schemastore.org/sarif-2.1.0.json".to_string(),
            version: "2.1.0".to_string(),
            runs: vec![Run {
                tool: Tool {
                    driver: Driver {
                        name: self.tool_name.clone(),
                        version: self.tool_version.clone(),
                        rules,
                    },
                },
                results,
            }],
        };

        Ok(serde_json::to_string_pretty(&output)?)
    }

    /// Formats matches into a rich HTML report.
    fn format_html(&self, matches: &[Match]) -> Result<String> {
        let mut html = String::new();

        html.push_str(r#"<!DOCTYPE html>
<html>
<head>
    <title>Oober Scanner Report</title>
    <style>
        body { font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; margin: 20px; }
        h1 { color: #333; }
        .summary { background: #f0f0f0; padding: 15px; border-radius: 5px; margin-bottom: 20px; }
        table { width: 100%; border-collapse: collapse; }
        th { background: #007bff; color: white; text-align: left; padding: 10px; }
        td { padding: 10px; border-bottom: 1px solid #ddd; }
        tr:hover { background: #f5f5f5; }
        .pattern { font-weight: bold; color: #d73a49; }
        .file { color: #0366d6; }
        .line-number { color: #6f42c1; }
        .content { font-family: 'Consolas', 'Monaco', monospace; background: #f6f8fa; padding: 5px; border-radius: 3px; }
        .severity-high { color: #d73a49; }
        .severity-medium { color: #fb8500; }
        .severity-low { color: #28a745; }
    </style>
</head>
<body>
    <h1>Oober Scanner Report</h1>
    <div class="summary">
        <strong>Total Matches:</strong> "#);

        html.push_str(&matches.len().to_string());
        html.push_str(
            r#"<br>
        <strong>Scan Time:</strong> "#,
        );
        html.push_str(&Utc::now().to_rfc3339());
        html.push_str(
            r#"<br>
        <strong>Tool Version:</strong> "#,
        );
        html.push_str(&self.tool_version);
        html.push_str(
            r#"
    </div>
    
    <table>
        <thead>
            <tr>
                <th>Pattern</th>
                <th>File</th>
                <th>Line</th>
                <th>Content</th>
                <th>Severity</th>
            </tr>
        </thead>
        <tbody>"#,
        );

        for m in matches {
            let severity = self.get_severity(&m.pattern_name);
            let severity_class = format!("severity-{}", severity.to_lowercase());

            html.push_str(&format!(
                r#"
            <tr>
                <td class="pattern">{}</td>
                <td class="file">{}</td>
                <td class="line-number">{}</td>
                <td><code class="content">{}</code></td>
                <td class="{}">{}</td>
            </tr>"#,
                html_escape(&m.pattern_name),
                html_escape(&m.file_path.display().to_string()),
                m.line_number,
                html_escape(m.line_content.trim()),
                severity_class,
                severity
            ));
        }

        html.push_str(
            r#"
        </tbody>
    </table>
</body>
</html>"#,
        );

        Ok(html)
    }

    /// Generates a summary of scan results, including counts and top patterns.
    fn format_summary(&self, matches: &[Match]) -> Result<String> {
        use std::collections::HashMap;

        let mut pattern_counts: HashMap<String, usize> = HashMap::new();
        let mut file_counts: HashMap<PathBuf, usize> = HashMap::new();

        for m in matches {
            *pattern_counts.entry(m.pattern_name.clone()).or_insert(0) += 1;
            *file_counts.entry(m.file_path.clone()).or_insert(0) += 1;
        }

        let mut summary = String::new();
        summary.push_str(&format!(
            "\n{} Summary {}\n",
            "=".repeat(20),
            "=".repeat(20)
        ));
        summary.push_str(&format!("Total matches: {}\n", matches.len()));
        summary.push_str(&format!("Files with matches: {}\n", file_counts.len()));
        summary.push_str(&format!("Unique patterns: {}\n\n", pattern_counts.len()));

        summary.push_str("Top patterns:\n");
        let mut patterns: Vec<_> = pattern_counts.iter().collect();
        patterns.sort_by(|a, b| b.1.cmp(a.1));

        for (pattern, count) in patterns.iter().take(10) {
            summary.push_str(&format!("  {} - {} matches\n", pattern, count));
        }

        Ok(summary)
    }

    /// Determines a severity level based on keywords in a pattern's name.
    ///
    /// # Optimization Note
    ///
    /// This is a simple heuristic. A more robust implementation would allow users
    /// to configure severities for patterns in the configuration file.
    fn get_severity(&self, pattern_name: &str) -> String {
        // Map pattern names to severity levels
        // This could be configurable
        // The keywords are matched in any case: `AWS_KEY` is a key too.
        let pattern_name = pattern_name.to_lowercase();
        if pattern_name.contains("secret") || pattern_name.contains("key") {
            "High".to_string()
        } else if pattern_name.contains("todo") || pattern_name.contains("fixme") {
            "Low".to_string()
        } else {
            "Medium".to_string()
        }
    }

    /// Maps the internal severity level to a SARIF-compliant level.
    fn get_sarif_level(&self, pattern_name: &str) -> String {
        match self.get_severity(pattern_name).as_str() {
            "High" => "error",
            "Medium" => "warning",
            _ => "note",
        }
        .to_string()
    }
}

/// Converts a matched file path into a SARIF `artifactLocation.uri`.
///
/// SARIF consumers expect URIs, not raw OS paths: the path is made relative
/// to the current working directory where possible and always uses forward
/// slashes (with any leading `./` dropped). A path outside the working
/// directory keeps its absolute form, slash-normalized as a best effort.
fn sarif_uri(path: &Path) -> String {
    let rel = std::env::current_dir()
        .ok()
        .and_then(|cwd| path.strip_prefix(cwd).ok())
        .unwrap_or(path);

    if rel.is_absolute() {
        let raw = rel.display().to_string();
        return match std::path::MAIN_SEPARATOR {
            '/' => raw,
            sep => raw.replace(sep, "/"),
        };
    }

    rel.components()
        .filter(|c| !matches!(c, Component::CurDir))
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

/// Guards a CSV field against spreadsheet formula injection.
///
/// A field starting with `=`, `+`, `-`, or `@` is executed as a formula by
/// Excel, Google Sheets, and LibreOffice when the CSV is opened. Matched line
/// content, file names, and pattern names all come from scanned input, so such
/// fields are prefixed with a single quote — the conventional spreadsheet
/// "treat as text" marker.
fn guard_csv_formula(field: &str) -> Cow<'_, str> {
    match field.chars().next() {
        Some('=' | '+' | '-' | '@') => Cow::Owned(format!("'{field}")),
        _ => Cow::Borrowed(field),
    }
}

/// Escapes a string for safe inclusion in HTML.
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_matches() -> Vec<Match> {
        vec![
            Match {
                pattern_name: "email".to_string(),
                file_path: PathBuf::from("src/main.rs"),
                line_number: 42,
                line_content: "let email = \"test@example.com\";".to_string(),
            },
            Match {
                pattern_name: "api_key".to_string(),
                file_path: PathBuf::from("config.toml"),
                line_number: 10,
                line_content: "api_key = \"sk-1234567890\"".to_string(),
            },
        ]
    }

    #[test]
    fn test_json_format() {
        let formatter = OutputFormatter::new(OutputFormat::Json, false);
        let matches = create_test_matches();

        let output = formatter.format_json(&matches).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();

        assert_eq!(parsed["total_matches"], 2);
        assert_eq!(parsed["matches"][0]["pattern"], "email");
    }

    #[test]
    fn test_csv_format() {
        let formatter = OutputFormatter::new(OutputFormat::Csv, false);
        let matches = create_test_matches();

        let output = formatter.format_csv(&matches).unwrap();

        // Parse CSV and verify
        let mut rdr = csv::Reader::from_reader(output.as_bytes());
        let headers = rdr.headers().unwrap();
        assert_eq!(headers.get(0), Some("Pattern"));

        let records: Vec<_> = rdr
            .records()
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(records.len(), 2);
    }

    #[test]
    fn test_guard_csv_formula_prefixes_only_leading_formula_chars() {
        for field in ["=x", "+x", "-x", "@x"] {
            assert_eq!(guard_csv_formula(field), format!("'{field}"));
        }
        assert_eq!(guard_csv_formula("x = y"), "x = y");
        assert_eq!(guard_csv_formula(""), "");
    }

    #[test]
    fn test_csv_formula_injection_is_neutralized() {
        let formatter = OutputFormatter::new(OutputFormat::Csv, false);
        let matches = vec![
            Match {
                pattern_name: "cmd".to_string(),
                file_path: PathBuf::from("evil.csv"),
                line_number: 1,
                line_content: "=HYPERLINK(\"http://evil\",\"click\")".to_string(),
            },
            Match {
                pattern_name: "cmd".to_string(),
                file_path: PathBuf::from("evil.csv"),
                line_number: 2,
                // Leading whitespace is trimmed before the guard applies.
                line_content: "  @SUM(1,2)".to_string(),
            },
            Match {
                pattern_name: "cmd".to_string(),
                file_path: PathBuf::from("ok.txt"),
                line_number: 3,
                line_content: "plain = safe".to_string(),
            },
            // The guard must cover every string field, not just Content: a
            // scanned file or pattern named like a formula injects through
            // its own column.
            Match {
                pattern_name: "=cmd".to_string(),
                file_path: PathBuf::from("=HYPERLINK(evil).txt"),
                line_number: 4,
                line_content: "harmless".to_string(),
            },
        ];

        let output = formatter.format_csv(&matches).unwrap();
        let mut rdr = csv::Reader::from_reader(output.as_bytes());
        let records: Vec<_> = rdr
            .records()
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap();

        assert_eq!(
            records[0].get(3),
            Some("'=HYPERLINK(\"http://evil\",\"click\")"),
            "leading = must be neutralized"
        );
        assert_eq!(
            records[1].get(3),
            Some("'@SUM(1,2)"),
            "leading @ must be neutralized"
        );
        assert_eq!(
            records[2].get(3),
            Some("plain = safe"),
            "an interior = must be left alone"
        );
        assert_eq!(
            records[3].get(0),
            Some("'=cmd"),
            "a formula-shaped pattern name must be neutralized"
        );
        assert_eq!(
            records[3].get(1),
            Some("'=HYPERLINK(evil).txt"),
            "a formula-shaped file name must be neutralized"
        );
        assert_eq!(
            records[2].get(1),
            Some("ok.txt"),
            "an ordinary file name must be left alone"
        );
    }

    #[test]
    fn test_sarif_format() {
        let formatter = OutputFormatter::new(OutputFormat::Sarif, false);
        let matches = create_test_matches();

        let output = formatter.format_sarif(&matches).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();

        assert_eq!(parsed["version"], "2.1.0");
        let results = parsed["runs"][0]["results"].as_array().unwrap();
        assert_eq!(results.len(), 2);

        // artifactLocation.uri must be a relative, forward-slash URI.
        assert_eq!(
            results[0]["locations"][0]["physicalLocation"]["artifactLocation"]["uri"],
            "src/main.rs"
        );
        assert_eq!(
            results[1]["locations"][0]["physicalLocation"]["artifactLocation"]["uri"],
            "config.toml"
        );
    }

    #[test]
    fn test_severity_ignores_pattern_name_case() {
        // The keyword checks were case-sensitive, so an upper-case pattern
        // name always fell through to Medium.
        let matches = vec![
            Match {
                pattern_name: "AWS_KEY".to_string(),
                file_path: PathBuf::from("config.toml"),
                line_number: 1,
                line_content: "key = AKIAABCDEFGHIJKLMNOP".to_string(),
            },
            Match {
                pattern_name: "TODO".to_string(),
                file_path: PathBuf::from("src/main.rs"),
                line_number: 2,
                line_content: "// TODO later".to_string(),
            },
        ];

        let json = OutputFormatter::new(OutputFormat::Json, false)
            .format_json(&matches)
            .unwrap();
        let json: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(json["matches"][0]["severity"], "High");
        assert_eq!(json["matches"][1]["severity"], "Low");

        let sarif = OutputFormatter::new(OutputFormat::Sarif, false)
            .format_sarif(&matches)
            .unwrap();
        let sarif: serde_json::Value = serde_json::from_str(&sarif).unwrap();
        let results = sarif["runs"][0]["results"].as_array().unwrap();
        assert_eq!(results[0]["level"], "error");
        assert_eq!(results[1]["level"], "note");
    }

    #[test]
    fn test_sarif_uri_relativizes_and_normalizes() {
        // An absolute path under the current working directory becomes relative.
        let cwd = std::env::current_dir().unwrap();
        assert_eq!(sarif_uri(&cwd.join("src").join("main.rs")), "src/main.rs");

        // A "./"-prefixed relative path (as produced when scanning ".") is cleaned.
        assert_eq!(sarif_uri(Path::new("./src/main.rs")), "src/main.rs");

        // A plain relative path is kept as-is.
        assert_eq!(sarif_uri(Path::new("config.toml")), "config.toml");

        // An absolute path outside the working directory stays absolute.
        #[cfg(unix)]
        assert_eq!(
            sarif_uri(Path::new("/outside/of/cwd.txt")),
            "/outside/of/cwd.txt"
        );
    }

    #[test]
    fn test_html_format() {
        let formatter = OutputFormatter::new(OutputFormat::Html, false);
        let matches = create_test_matches();

        let output = formatter.format_html(&matches).unwrap();

        assert!(output.contains("<!DOCTYPE html>"));
        assert!(output.contains("test@example.com"));
        assert!(output.contains("sk-1234567890"));
    }

    #[test]
    fn test_html_escaping() {
        let dangerous = "< script>alert('xss')</script>";
        let escaped = html_escape(dangerous);

        assert_eq!(
            escaped,
            "&lt; script&gt;alert(&#39;xss&#39;)&lt;/script&gt;"
        );
    }
}

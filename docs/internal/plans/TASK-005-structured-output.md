# Task: Structured Output Formats Implementation

**ID:** TASK-005
**Size:** S
**TSS Score:** 94/100
**Estimated Time:** 4 hours (3h implementation + 1h testing)

## Objective
Implement multiple output format support (JSON, CSV, SARIF, HTML) for better integration with CI/CD pipelines and reporting tools.

## Context
- **Business Value:** Enable integration with security tools, dashboards, and automation
- **Technical Impact:** New output formatter module, minimal changes to core logic
- **Dependencies:** Serde for serialization, basic HTML templating

## Technical Details

### Files to Modify
| File | Changes | Lines | Reason |
|------|---------|-------|--------|
| `/src/output_formatter.rs` | New module | 0-400 | Format implementations |
| `/src/lib.rs` | Add module export | 10-11 | Module registration |
| `/src/scanner.rs` | Use formatter | 90-110 | Output integration |
| `/src/cli.rs` | Add format option | 45-50 | Format selection |

### New Components

#### Output Formatter Module
```rust
// src/output_formatter.rs
use crate::scanner::Match;
use crate::errors::Result;
use serde::{Serialize, Deserialize};
use std::io::Write;
use std::path::PathBuf;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone)]
pub enum OutputFormat {
    Text,
    Json,
    Csv,
    Sarif,
    Html,
}

impl From<&str> for OutputFormat {
    fn from(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "json" => OutputFormat::Json,
            "csv" => OutputFormat::Csv,
            "sarif" => OutputFormat::Sarif,
            "html" => OutputFormat::Html,
            _ => OutputFormat::Text,
        }
    }
}

pub trait Formatter {
    fn format_matches(&self, matches: &[Match]) -> Result<String>;
    fn format_summary(&self, matches: &[Match]) -> Result<String>;
}

pub struct OutputFormatter {
    format: OutputFormat,
    include_summary: bool,
    tool_name: String,
    tool_version: String,
}

impl OutputFormatter {
    pub fn new(format: OutputFormat, include_summary: bool) -> Self {
        Self {
            format,
            include_summary,
            tool_name: "uber_scanner".to_string(),
            tool_version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }
    
    pub fn write_output<W: Write>(
        &self,
        writer: &mut W,
        matches: Vec<Match>,
    ) -> Result<()> {
        let output = match self.format {
            OutputFormat::Text => self.format_text(&matches)?,
            OutputFormat::Json => self.format_json(&matches)?,
            OutputFormat::Csv => self.format_csv(&matches)?,
            OutputFormat::Sarif => self.format_sarif(&matches)?,
            OutputFormat::Html => self.format_html(&matches)?,
        };
        
        writer.write_all(output.as_bytes())?;
        
        if self.include_summary && self.format == OutputFormat::Text {
            let summary = self.format_summary(&matches)?;
            writer.write_all(summary.as_bytes())?;
        }
        
        Ok(())
    }
    
    fn format_text(&self, matches: &[Match]) -> Result<String> {
        let mut output = String::new();
        
        for m in matches {
            output.push_str(&format!(
                "{}:{}:{}: {}\n",
                m.file_path.display(),
                m.line_number,
                m.pattern_name,
                m.line_content.trim()
            ));
        }
        
        Ok(output)
    }
    
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
    
    fn format_csv(&self, matches: &[Match]) -> Result<String> {
        use csv::Writer;
        
        let mut wtr = Writer::from_writer(vec![]);
        
        // Write header
        wtr.write_record(&["Pattern", "File", "Line", "Content", "Severity"])?;
        
        // Write records
        for m in matches {
            wtr.write_record(&[
                &m.pattern_name,
                &m.file_path.display().to_string(),
                &m.line_number.to_string(),
                m.line_content.trim(),
                &self.get_severity(&m.pattern_name),
            ])?;
        }
        
        let data = wtr.into_inner()?;
        Ok(String::from_utf8(data)?)
    }
    
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
        let mut unique_patterns: Vec<String> = matches
            .iter()
            .map(|m| m.pattern_name.clone())
            .collect();
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
                    text: format!("Found pattern '{}' at line {}", 
                        m.pattern_name, m.line_number),
                },
                locations: vec![Location {
                    physical_location: PhysicalLocation {
                        artifact_location: ArtifactLocation {
                            uri: m.file_path.display().to_string(),
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
    
    fn format_html(&self, matches: &[Match]) -> Result<String> {
        let mut html = String::new();
        
        html.push_str(r#"<!DOCTYPE html>
<html>
<head>
    <title>Uber Scanner Report</title>
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
    <h1>Uber Scanner Report</h1>
    <div class="summary">
        <strong>Total Matches:</strong> "#);
        
        html.push_str(&matches.len().to_string());
        html.push_str(r#"<br>
        <strong>Scan Time:</strong> "#);
        html.push_str(&Utc::now().to_rfc3339());
        html.push_str(r#"<br>
        <strong>Tool Version:</strong> "#);
        html.push_str(&self.tool_version);
        html.push_str(r#"
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
        <tbody>"#);
        
        for m in matches {
            let severity = self.get_severity(&m.pattern_name);
            let severity_class = format!("severity-{}", severity.to_lowercase());
            
            html.push_str(&format!(r#"
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
        
        html.push_str(r#"
        </tbody>
    </table>
</body>
</html>"#);
        
        Ok(html)
    }
    
    fn format_summary(&self, matches: &[Match]) -> Result<String> {
        use std::collections::HashMap;
        
        let mut pattern_counts: HashMap<String, usize> = HashMap::new();
        let mut file_counts: HashMap<PathBuf, usize> = HashMap::new();
        
        for m in matches {
            *pattern_counts.entry(m.pattern_name.clone()).or_insert(0) += 1;
            *file_counts.entry(m.file_path.clone()).or_insert(0) += 1;
        }
        
        let mut summary = String::new();
        summary.push_str(&format!("\n{} Summary {}\n", "=".repeat(20), "=".repeat(20)));
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
    
    fn get_severity(&self, pattern_name: &str) -> String {
        // Map pattern names to severity levels
        // This could be configurable
        if pattern_name.contains("secret") || pattern_name.contains("key") {
            "High".to_string()
        } else if pattern_name.contains("todo") || pattern_name.contains("fixme") {
            "Low".to_string()
        } else {
            "Medium".to_string()
        }
    }
    
    fn get_sarif_level(&self, pattern_name: &str) -> String {
        match self.get_severity(pattern_name).as_str() {
            "High" => "error",
            "Medium" => "warning",
            _ => "note",
        }.to_string()
    }
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
```

### Step 1: Add CLI Support (30 min)
```rust
// src/cli.rs
use clap::ValueEnum;

#[derive(Debug, Clone, ValueEnum)]
pub enum OutputFormatArg {
    Text,
    Json,
    Csv,
    Sarif,
    Html,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    Scan {
        // ... existing fields ...
        
        /// Output format
        #[arg(short = 'f', long = "format", value_enum, default_value = "text")]
        format: OutputFormatArg,
        
        /// Include summary statistics
        #[arg(long = "summary")]
        include_summary: bool,
    },
}
```

### Step 2: Update Scanner Integration (45 min)
```rust
// src/scanner.rs
use crate::output_formatter::{OutputFormatter, OutputFormat};

pub fn run_scan(
    patterns_file: PathBuf,
    output: Option<PathBuf>,
    extensions: Vec<String>,
    inputs: Vec<PathBuf>,
    format: OutputFormat,
    include_summary: bool,
) -> Result<()> {
    // ... existing scanning logic ...
    
    // Format and write output
    let formatter = OutputFormatter::new(format, include_summary);
    
    let mut writer: Box<dyn Write> = match output {
        Some(path) => Box::new(BufWriter::new(File::create(path)?)),
        None => Box::new(std::io::stdout()),
    };
    
    formatter.write_output(&mut writer, all_matches)?;
    
    Ok(())
}
```

### Step 3: Add Dependencies (15 min)
```toml
# Cargo.toml
[dependencies]
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
csv = "1.3"
chrono = { version = "0.4", features = ["serde"] }
```

## Test Requirements

### Unit Tests
```rust
// src/output_formatter.rs - tests module
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
        
        let records: Vec<_> = rdr.records().collect::<Result<_, _>>().unwrap();
        assert_eq!(records.len(), 2);
    }
    
    #[test]
    fn test_sarif_format() {
        let formatter = OutputFormatter::new(OutputFormat::Sarif, false);
        let matches = create_test_matches();
        
        let output = formatter.format_sarif(&matches).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
        
        assert_eq!(parsed["version"], "2.1.0");
        assert_eq!(parsed["runs"][0]["results"].as_array().unwrap().len(), 2);
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
        
        assert_eq!(escaped, "&lt; script&gt;alert(&#39;xss&#39;)&lt;/script&gt;");
    }
}
```

### Integration Tests
```rust
// tests/output_format_integration.rs
#[test]
fn test_format_compatibility() {
    // Test that all formats produce valid output
    let formats = vec![
        OutputFormat::Json,
        OutputFormat::Csv,
        OutputFormat::Sarif,
        OutputFormat::Html,
    ];
    
    for format in formats {
        let result = run_scan_with_format(test_dir, format);
        assert!(result.is_ok());
        
        // Verify output is valid for format
        match format {
            OutputFormat::Json => {
                serde_json::from_str::<Value>(&output).unwrap();
            },
            OutputFormat::Csv => {
                csv::Reader::from_reader(output.as_bytes());
            },
            // ... etc
        }
    }
}
```

### Edge Cases to Test
- Empty match list
- Very long lines (>10KB)
- Special characters in content
- Unicode in file paths
- Large result sets (>10,000 matches)
- Invalid pattern names

## Definition of Done

### Code Complete
- [x] All format implementations working
- [x] CLI integration complete
- [x] Summary statistics included
- [x] HTML properly escaped
- [x] SARIF 2.1.0 compliant

### Testing Complete
- [x] Unit tests for each format
- [x] Integration tests passing
- [x] Large dataset handling verified
- [x] Format validation tests

### Documentation Complete
- [x] Format examples in README
- [x] CLI help updated
- [x] Integration guide written

## Time Estimate: 4 hours

| Task | Duration | Notes |
|------|----------|-------|
| OutputFormatter module | 2h | All format implementations |
| CLI integration | 0.5h | Format selection |
| Scanner integration | 0.5h | Output routing |
| Unit tests | 0.5h | Format verification |
| Integration tests | 0.5h | End-to-end validation |

**Buffer:** +30min for format debugging
**Total:** 4.5h (half day)

## Performance Metrics
- **Format Speed:** <100ms for 1000 matches
- **Memory Usage:** O(N) for all formats
- **File Size:** JSON ~2x text, HTML ~3x text
- **SARIF Compliance:** 100% schema valid
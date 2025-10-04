#!/bin/bash
# run.sh - Analyze Rails logs with uber_scanner

# Configuration
SCANNER_DIR="."
LOG_FILE="/Users/macadelic/mangroves/workmac/mangrove/lib/packages/rust/uber_scanner/prod.log"
PATTERNS_FILE="$SCANNER_DIR/human_analytics_patterns.yaml"
SCANNER_BIN="$SCANNER_DIR/target/release/uber_scanner"

# Check if scanner is built
if [ ! -f "$SCANNER_BIN" ]; then
    echo "Error: uber_scanner not built yet!"
    echo "Building uber_scanner..."
    cd "$SCANNER_DIR"
    cargo build --release
    if [ $? -ne 0 ]; then
        echo "Build failed! Please check Rust installation."
        exit 1
    fi
fi

# Check if files exist
if [ ! -f "$LOG_FILE" ]; then
    echo "Error: Log file not found at $LOG_FILE"
    exit 1
fi

if [ ! -f "$PATTERNS_FILE" ]; then
    echo "Error: Patterns file not found at $PATTERNS_FILE"
    echo "Please save the human_analytics_patterns.yaml file in the uber_scanner directory"
    exit 1
fi

OUTPUT_DIR="analysis_$(date +%Y%m%d_%H%M%S)"
mkdir -p "$OUTPUT_DIR"

echo "Analyzing Rails logs with uber_scanner..."
echo "Log file: $LOG_FILE"
echo "Output directory: $OUTPUT_DIR"

# Run the scan
"$SCANNER_BIN" scan \
  -p "$PATTERNS_FILE" \
  -o "$OUTPUT_DIR/raw_analysis.txt" \
  "$LOG_FILE"

if [ $? -ne 0 ]; then
    echo "Scan failed!"
    exit 1
fi

# Generate reports
echo "Generating reports..."

# Pattern frequency
awk -F'[][]' '{print $2}' "$OUTPUT_DIR/raw_analysis.txt" | \
  sort | uniq -c | sort -rn > "$OUTPUT_DIR/pattern_frequency.txt"

# Unique visitors (excluding bot patterns)
grep -v "exclude_" "$OUTPUT_DIR/raw_analysis.txt" | \
  grep -oE 'for [0-9.]+' | cut -d' ' -f2 | \
  sort -u > "$OUTPUT_DIR/unique_visitors.txt"

# Conversion events
grep -E "donation_success|angel_submission|user_signup_attempt" \
  "$OUTPUT_DIR/raw_analysis.txt" > "$OUTPUT_DIR/conversions.txt"

# Performance issues
grep -E "slow_request_warning|very_slow_request|server_error_500" \
  "$OUTPUT_DIR/raw_analysis.txt" > "$OUTPUT_DIR/performance_issues.txt"

# Generate summary
echo "Analysis complete! Results in $OUTPUT_DIR/"
echo "Total patterns found: $(wc -l < "$OUTPUT_DIR/raw_analysis.txt")"
echo "Unique visitors: $(wc -l < "$OUTPUT_DIR/unique_visitors.txt")"
echo ""
echo "Top 10 patterns:"
head -10 "$OUTPUT_DIR/pattern_frequency.txt"
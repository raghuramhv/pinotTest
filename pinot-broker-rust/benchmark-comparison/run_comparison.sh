#!/bin/bash
#
# Rust vs Java Broker Benchmark Comparison
# This script runs equivalent benchmarks in both Rust and Java,
# then compares the results.
#

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
RUST_DIR="$(dirname "$SCRIPT_DIR")"
RESULTS_DIR="$SCRIPT_DIR/results"

mkdir -p "$RESULTS_DIR"

echo "=============================================="
echo "Pinot Broker: Rust vs Java Benchmark"
echo "=============================================="
echo ""

# ==================== Build Java Benchmark ====================
echo "Building Java JMH benchmark..."
cd "$SCRIPT_DIR"
mvn clean package -q -DskipTests 2>/dev/null || {
    echo "Maven build failed. Ensure Maven is installed."
    exit 1
}
echo "Java build complete."
echo ""

# ==================== Build Rust Benchmark ====================
echo "Building Rust benchmark..."
cd "$RUST_DIR"
cargo build --release --quiet 2>/dev/null
echo "Rust build complete."
echo ""

# ==================== Run Java Benchmark ====================
echo "=============================================="
echo "Running Java JMH Benchmark..."
echo "=============================================="
cd "$SCRIPT_DIR"

java -jar target/benchmarks.jar \
    -f 1 \
    -wi 3 \
    -i 5 \
    -r 3s \
    -w 2s \
    -tu us \
    -rf json \
    -rff "$RESULTS_DIR/java_results.json" \
    2>&1 | tee "$RESULTS_DIR/java_output.txt"

echo ""
echo "Java benchmark complete. Results saved to $RESULTS_DIR/java_results.json"
echo ""

# ==================== Run Rust Benchmark ====================
echo "=============================================="
echo "Running Rust Criterion Benchmark..."
echo "=============================================="
cd "$RUST_DIR"

# Run Rust benchmarks and capture output
cargo bench --bench routing -- --noplot 2>&1 | tee "$RESULTS_DIR/rust_routing.txt"
cargo bench --bench reduce -- --noplot 2>&1 | tee "$RESULTS_DIR/rust_reduce.txt"

echo ""
echo "Rust benchmark complete."
echo ""

# ==================== Parse and Compare Results ====================
echo "=============================================="
echo "Parsing Results..."
echo "=============================================="

# Create comparison report
cat > "$RESULTS_DIR/comparison_report.md" << 'EOF'
# Rust vs Java Broker Benchmark Comparison Report

## Test Environment
- Date: $(date)
- Platform: $(uname -a)
- Java: $(java -version 2>&1 | head -1)
- Rust: $(rustc --version)

## Results Summary

EOF

# Parse Java results
echo "### Java Results" >> "$RESULTS_DIR/comparison_report.md"
echo "" >> "$RESULTS_DIR/comparison_report.md"
echo '```' >> "$RESULTS_DIR/comparison_report.md"
grep -E "Benchmark|Mode|Cnt|Score|Error" "$RESULTS_DIR/java_output.txt" | head -50 >> "$RESULTS_DIR/comparison_report.md" || echo "No Java results found" >> "$RESULTS_DIR/comparison_report.md"
echo '```' >> "$RESULTS_DIR/comparison_report.md"
echo "" >> "$RESULTS_DIR/comparison_report.md"

# Parse Rust results
echo "### Rust Results" >> "$RESULTS_DIR/comparison_report.md"
echo "" >> "$RESULTS_DIR/comparison_report.md"
echo '```' >> "$RESULTS_DIR/comparison_report.md"
grep -E "time:|throughput" "$RESULTS_DIR/rust_routing.txt" "$RESULTS_DIR/rust_reduce.txt" | head -50 >> "$RESULTS_DIR/comparison_report.md" || echo "No Rust results found" >> "$RESULTS_DIR/comparison_report.md"
echo '```' >> "$RESULTS_DIR/comparison_report.md"

echo ""
echo "=============================================="
echo "Benchmark Complete!"
echo "=============================================="
echo ""
echo "Results saved to: $RESULTS_DIR/"
echo "  - java_results.json    (Java JMH JSON output)"
echo "  - java_output.txt      (Java console output)"
echo "  - rust_routing.txt     (Rust routing benchmarks)"
echo "  - rust_reduce.txt      (Rust reduce benchmarks)"
echo "  - comparison_report.md (Summary report)"
echo ""

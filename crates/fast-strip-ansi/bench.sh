#!/bin/bash

# Change to the fast-strip-ansi directory
cd "$(dirname "$0")"

echo "Running benchmarks from fast-strip-ansi directory..."
echo "=================================================="

# Run benchmarks from the benches directory and filter for _0 and _100
cd ../benches
cargo bench | grep -E "_0|_100" > /tmp/bench_results.txt

# Parse the results and extract the performance data
echo "Parsing benchmark results..."
echo "============================"

# Display the raw results first
echo "Raw benchmark results:"
echo "======================"
cat /tmp/bench_results.txt
echo ""

# Parse the results to extract mean values
echo "Raw performance data:"
echo "===================="
echo ""
echo "_from \`cargo bench\` on an M3 MacBook Pro_"
echo ""
echo "| comparison                         | fastest  | slowest  | median   | mean     |"

# Parse each line to extract the benchmark name and statistics
while IFS= read -r line; do
    if [[ $line =~ [├╰]─[[:space:]]+([a-zA-Z_]+_[0-9]+)[[:space:]]+([0-9]+\.[0-9]+)[[:space:]]+µs ]]; then
        benchmark_name="${BASH_REMATCH[1]}"
        
        # Extract all four statistics from the line
        # Format: fastest │ slowest │ median │ mean
        if [[ $line =~ ([0-9]+\.[0-9]+)[[:space:]]+µs[[:space:]]+│[[:space:]]+([0-9]+\.[0-9]+)[[:space:]]+µs[[:space:]]+│[[:space:]]+([0-9]+\.[0-9]+)[[:space:]]+µs[[:space:]]+│[[:space:]]+([0-9]+\.[0-9]+)[[:space:]]+µs ]]; then
            fastest="${BASH_REMATCH[1]}"
            slowest="${BASH_REMATCH[2]}"
            median="${BASH_REMATCH[3]}"
            mean="${BASH_REMATCH[4]}"
            echo "| $benchmark_name | ${fastest} µs | ${slowest} µs | ${median} µs | ${mean} µs |"
        fi
    fi
done < /tmp/bench_results.txt

echo ""

# Generate the mermaid chart
echo "Mermaid chart:"
echo "=============="
echo ""

# Extract values for the chart by parsing the results again
fast_strip_ansi_100=""
fast_strip_ansi_callback_100=""
strip_ansi_100=""
strip_ansi_escapes_100=""
fast_strip_ansi_0=""
fast_strip_ansi_callback_0=""
strip_ansi_0=""
strip_ansi_escapes_0=""

while IFS= read -r line; do
    if [[ $line =~ [├╰]─[[:space:]]+([a-zA-Z_]+_[0-9]+)[[:space:]]+([0-9]+\.[0-9]+)[[:space:]]+µs ]]; then
        benchmark_name="${BASH_REMATCH[1]}"
        
        # Extract the mean value (last number in the line)
        if [[ $line =~ ([0-9]+\.[0-9]+)[[:space:]]+µs[[:space:]]+│[[:space:]]+([0-9]+\.[0-9]+)[[:space:]]+µs[[:space:]]+│[[:space:]]+([0-9]+\.[0-9]+)[[:space:]]+µs[[:space:]]+│[[:space:]]+([0-9]+\.[0-9]+)[[:space:]]+µs ]]; then
            mean_time="${BASH_REMATCH[4]}"
            
            case "$benchmark_name" in
                "fast_strip_ansi_crate_100")
                    fast_strip_ansi_100="$mean_time"
                    ;;
                "fast_strip_ansi_crate_callback_100")
                    fast_strip_ansi_callback_100="$mean_time"
                    ;;
                "strip_ansi_crate_100")
                    strip_ansi_100="$mean_time"
                    ;;
                "strip_ansi_escapes_crate_100")
                    strip_ansi_escapes_100="$mean_time"
                    ;;
                "fast_strip_ansi_crate_0")
                    fast_strip_ansi_0="$mean_time"
                    ;;
                "fast_strip_ansi_crate_callback_0")
                    fast_strip_ansi_callback_0="$mean_time"
                    ;;
                "strip_ansi_crate_0")
                    strip_ansi_0="$mean_time"
                    ;;
                "strip_ansi_escapes_crate_0")
                    strip_ansi_escapes_0="$mean_time"
                    ;;
                "strip_ansi_escapes_crate_100")
                    strip_ansi_escapes_100="$mean_time"
                    ;;
            esac
        fi
    fi
done < /tmp/bench_results.txt

echo '```mermaid'
echo 'xychart'
echo '    title "Performance"'
echo '    x-axis [fast-strip-ansi, fast-strip-ansi-callback, strip-ansi, strip-ansi-escapes]'
echo '    y-axis "Time (in µs)" 0 --> 100'

# Use the extracted values for the chart
echo "    bar [$fast_strip_ansi_100, $fast_strip_ansi_callback_100, $strip_ansi_100, $strip_ansi_escapes_100]"
echo "    bar [$fast_strip_ansi_0, $fast_strip_ansi_callback_0, $strip_ansi_0, $strip_ansi_escapes_0]"
echo '```'

echo ""
echo "Benchmark script completed!"

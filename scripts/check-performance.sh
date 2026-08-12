#!/usr/bin/env bash
set -euo pipefail
# Wall-clock budgets are measured in isolation; running the two large workbook
# tests concurrently measures scheduler contention rather than either parser.
cargo test --release -p readany-render --test performance -- --nocapture --test-threads=1

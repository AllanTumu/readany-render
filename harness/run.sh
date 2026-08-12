#!/usr/bin/env bash
set -euo pipefail
cargo run --release -p readany-render-harness -- "$@"


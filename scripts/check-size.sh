#!/usr/bin/env bash
set -euo pipefail

cargo build -p readany-render-wasm --target wasm32-unknown-unknown --release --no-default-features
wasm="target/wasm32-unknown-unknown/release/readany_render_wasm.wasm"
gzip -9 -c "$wasm" > "$wasm.gz"
bytes=$(wc -c < "$wasm.gz" | tr -d ' ')
limit=$((4 * 1024 * 1024))
echo "core_wasm_gzip_bytes=$bytes"
if [ "$bytes" -ge "$limit" ]; then
  echo "core WASM exceeds the 4 MiB gzip budget" >&2
  exit 1
fi

cargo build -p readany-render-wasm --target wasm32-unknown-unknown --release --features fonts
gzip -9 -c "$wasm" > "$wasm.fonts.gz"
font_bytes=$(wc -c < "$wasm.fonts.gz" | tr -d ' ')
font_limit=$((9 * 1024 * 1024))
echo "bundled_wasm_gzip_bytes=$font_bytes"
if [ "$font_bytes" -ge "$font_limit" ]; then
  echo "bundled-font WASM exceeds the 9 MiB gzip budget" >&2
  exit 1
fi

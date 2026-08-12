#!/usr/bin/env bash
set -euo pipefail

mode=${1:-bundled}
out_dir=${2:-pkg}
features=()
if [ "$mode" = "bundled" ]; then
  features=(--features fonts)
elif [ "$mode" != "none" ]; then
  echo "usage: $0 [bundled|none] [output-directory]" >&2
  exit 2
fi
wasm-pack build crates/readany-render-wasm --release --target web --out-dir "../../$out_dir" "${features[@]}"
cp crates/readany-render-wasm/readany_render_wasm.d.ts "$out_dir/readany_render_wasm.d.ts"
cp crates/readany-render-wasm/package.json "$out_dir/package.json"
cp README.md "$out_dir/README.md"
mkdir -p "$out_dir/font-licenses"
cp crates/readany-render/fonts/Caladea/OFL.txt "$out_dir/font-licenses/Caladea-OFL.txt"
cp crates/readany-render/fonts/Carlito/OFL.txt "$out_dir/font-licenses/Carlito-OFL.txt"
cp crates/readany-render/fonts/DejaVu/LICENSE.txt "$out_dir/font-licenses/DejaVu-LICENSE.txt"
cp crates/readany-render/fonts/Liberation/OFL.txt "$out_dir/font-licenses/Liberation-OFL.txt"

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
# **The version lives in the template, never in pkg/.**
#
# This copy overwrites whatever pkg/package.json says, so editing the version
# there is silently undone by the next build — which is exactly what happened
# between 0.1.1 and 0.1.2. Fail loudly when the template disagrees with the
# workspace rather than shipping a package whose version is a leftover.
crate_version=$(grep -m1 '^version = ' Cargo.toml | sed 's/.*"\(.*\)".*/\1/')
npm_version=$(grep -m1 '"version"' crates/readany-render-wasm/package.json | sed 's/.*"\([0-9][^"]*\)".*/\1/')
if [ "$crate_version" != "$npm_version" ]; then
  echo "build-wasm: crates/readany-render-wasm/package.json is $npm_version but the workspace is $crate_version." >&2
  echo "build-wasm: bump the template, not pkg/package.json, which this script overwrites." >&2
  exit 1
fi
cp crates/readany-render-wasm/package.json "$out_dir/package.json"
cp README.md "$out_dir/README.md"
mkdir -p "$out_dir/font-licenses"
cp crates/readany-render/fonts/Caladea/OFL.txt "$out_dir/font-licenses/Caladea-OFL.txt"
cp crates/readany-render/fonts/Carlito/OFL.txt "$out_dir/font-licenses/Carlito-OFL.txt"
cp crates/readany-render/fonts/DejaVu/LICENSE.txt "$out_dir/font-licenses/DejaVu-LICENSE.txt"
cp crates/readany-render/fonts/Liberation/OFL.txt "$out_dir/font-licenses/Liberation-OFL.txt"

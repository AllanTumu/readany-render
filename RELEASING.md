# Releasing

The owner performs releases. Automation prepares artifacts but never publishes.

```bash
python3 fixtures/generate.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
./scripts/check-performance.sh
./scripts/check-size.sh
./harness/run.sh
cargo package -p readany-render
./scripts/build-wasm.sh bundled pkg
cd pkg && npm pack --dry-run && cd ..
```

After reviewing package contents and the changelog:

```bash
cargo publish -p readany-render
cd pkg
npm publish --access public
```

Do not execute either publish command from CI.

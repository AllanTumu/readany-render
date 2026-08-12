# Releasing

The owner performs releases from `main`. Automation prepares artifacts but never
publishes. The commands under **Publish** are recorded for an eventual owner-run
release and were deliberately not executed while preparing 0.1.0.

Before assigning a version, read the per-format table in `README.md` and the
measured defects in `docs/FIDELITY.md`. A release must not turn “implemented” or
synthetic-only evidence into a uniform fidelity claim.

## Validate and stage

```bash
python3 fixtures/generate.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
./scripts/check-performance.sh
./scripts/check-size.sh
./harness/run.sh
cargo check --manifest-path fuzz/Cargo.toml --bins
cargo package -p readany-render
./scripts/build-wasm.sh bundled pkg
cd pkg && npm pack --dry-run && cd ..
```

Confirm that `cargo package` contains the root README and license, that the npm
tarball contains the WASM, handwritten declarations, README, license, and every
font license, and that neither package contains corpus documents or harness
reports. Confirm the 4 MiB core and 9 MiB bundled gzip gates on the release
machine. Review `CHANGELOG.md`, commit the generated package provenance, and tag
the exact validated commit.

## Publish — owner only, not run during preparation

After confirming that version 0.1.0 is unused on both registries and reviewing
the package contents again, the exact publication commands are:

```bash
cargo publish -p readany-render
npm publish ./pkg --access public
```

Do not execute either publish command from CI or from an agent-run preparation.
If the first registry succeeds and the second fails, stop and report the split
release; do not change versions or republish opportunistically.

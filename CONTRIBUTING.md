# Contributing

Use the pinned Rust toolchain and public enzUI dependencies. Keep Cargo.lock
committed and update SDK/foundation pins deliberately. Shared widget defects
belong in enzUI rather than app-specific rendering copies.

```sh
cargo fmt --all -- --check
cargo test --locked --workspace
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
```

Keep originals read-only. Perform file access, decoding and export on bounded
workers. Async replies must retain their photo/session generation; cancelled or
retired work cannot replace the active image or publish a partial file.

Retain tests for editing semantics, original preservation, persistence custody,
transfer isolation and stale-result rejection. Prefer table-driven cases and
shared workflows over one test for each setter or incidental layout value.
Native UI checks must use disposable libraries and state. Check scrolling,
viewer/editing, drag and drop, both color modes and a small window.

This release does not ship installers or a CI workflow. Personal resource
limits and temporary review artifacts belong outside tracked files. Contributions
are under MIT; preserve third-party notices when vendoring source or assets.

Translation catalogs live in `assets/locales/`; keep English, German and Arabic
message keys aligned when adding or changing user-facing text.

# Photos contributor notes

Read README.md and the relevant format/service documentation. Keep photo editing,
scanning and persistence in Rust; reuse the pinned public enzUI framework.

Originals are read-only. Imports and exports publish new files atomically and
never overwrite existing media. Keep file I/O and full-resolution decoding off
the UI thread. Use bounded workers and reject asynchronous results whose owning
generation has retired.

Run formatting, targeted behavior tests and strict Clippy for code changes.
Tests must protect behavior, persistence and file safety, not source spelling or
one visual default. Test against sandbox libraries, never the user's originals.

Use isolated XDG paths for native checks. Capture actual app windows and inspect
light/dark themes, gallery scrolling, viewer and editor. Keep temporary captures,
photo libraries and review notes under ignored AGENTS/. Only curated README
screenshots belong in docs/screenshots/. Personal build policies belong outside
the tracked repository; reuse a compatible target directory without copying
artifacts or editing Cargo fingerprints.

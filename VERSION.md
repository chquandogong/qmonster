# Versioning

Qmonster has three version surfaces. They intentionally serve different
audiences.

| Surface | Current | Meaning |
| --- | --- | --- |
| Mission ledger | `v1.16.52` | Operator-facing implementation/history version. This is what tags and `mission-history.yaml` track. |
| npm package | `0.5.0` | Installable package version for the npm registry. |
| Cargo crate | `0.1.0` | Internal Rust crate metadata. It is not the operator-facing runtime version. |

The running TUI displays `git describe --tags --always --dirty`, captured
by `build.rs` as `QMONSTER_GIT_VERSION`. Tagged source builds therefore
show tags such as `v1.16.52`; dirty local builds show a `-dirty` suffix.
When the source is built outside a git checkout, the fallback label is
`v{CARGO_PKG_VERSION}-nogit`.

Release flow:

1. Update code, docs, `mission.yaml`, and `mission-history.yaml`.
2. Run the validation gates from `docs/ai/VALIDATION.md`.
3. Commit, push `main`, and create an annotated `vX.Y.Z` ledger tag.
4. Pack/publish the npm package when package metadata changed.

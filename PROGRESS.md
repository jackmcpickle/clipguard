# Progress

## Completed

- Windows backend added: deps, cfg gates, `clipboard_windows.rs`, registry app listing.
- Windows app IDs normalized to lowercase `.exe` in foreground detection + registry parsing.
- App picker now supports manual entry even with populated list.
- Windows manual entry validation added: must be `.exe`, no path; inline error shown.
- Enter behavior updated: exact list match first, else manual entry.
- `docs/windows-clipboard-plan.md` aligned to always-on hook + manual-entry behavior.
- Added CI lint workflow: `.github/workflows/lint.yml`.
- React CI checks run `pnpm fmt:check` and `pnpm lint`.
- Rust CI checks run `rustup component add rustfmt`, `cargo build`, `cargo test`, and `cargo fmt --all -- --check`.

## Remaining

- Runtime test on Windows for real hook behavior, app detection, and block-rule matching.
- Optional: improve blocker thread wake-up path for faster enable/disable responsiveness.
- Confirm new lint workflow passes in GitHub Actions on next push/PR.

## Decisions

- Keep aggressive registry filter.
- Keep hook strategy: always installed + atomic flag.
- Keep manual picker entry even when list exists.
- Require `.exe` for Windows manual app IDs.
- Use separate `Lint` workflow for React + Rust checks.

## Next Step

- Push branch / open PR and verify `Lint` workflow green.

# Progress

## Completed
- Windows backend added: deps, cfg gates, `clipboard_windows.rs`, registry app listing.
- Windows app IDs normalized to lowercase `.exe` in foreground detection + registry parsing.
- App picker now supports manual entry even with populated list.
- Windows manual entry validation added: must be `.exe`, no path; inline error shown.
- Enter behavior updated: exact list match first, else manual entry.
- `docs/windows-clipboard-plan.md` aligned to always-on hook + manual-entry behavior.

## Remaining
- Runtime test on Windows for real hook behavior, app detection, and block-rule matching.
- Optional: improve blocker thread wake-up path for faster enable/disable responsiveness.

## Decisions
- Keep aggressive registry filter.
- Keep hook strategy: always installed + atomic flag.
- Keep manual picker entry even when list exists.
- Require `.exe` for Windows manual app IDs.

## Next Step
- Run Windows E2E checks; if toggle lag appears, rework blocker loop wake-up.

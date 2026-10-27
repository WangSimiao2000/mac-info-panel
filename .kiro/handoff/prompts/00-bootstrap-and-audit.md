# Prompt 00: Bootstrap and audit

Work in `/Volumes/exSSD/Projects/mac-info-panel`.

Read `AGENTS.md`, `HANDOFF.md`, and all files in `.kiro/specs/rust-cross-platform-rewrite/` before changing anything. Confirm the current branch is `main`, inspect the working tree, and confirm `legacy/swift` points to the final pure Swift commit. Do not merge or copy Swift files into `main`.

Run `./Scripts/check-rust.sh` as the baseline. If it fails, diagnose and fix only regressions in the current Rust baseline before taking another task. Do not update dependencies merely because newer versions exist.

Return:

- current HEAD and branch status
- whether the baseline passes
- architecture summary
- the next unblocked task from `tasks.md`
- any mismatch between code, specs, and `HANDOFF.md`

Do not implement a feature during this audit unless needed to restore the recorded baseline.

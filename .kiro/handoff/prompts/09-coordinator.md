# Prompt 09: KiroCrew coordinator

Act as the integration coordinator for Info Panel. Read `AGENTS.md`, `HANDOFF.md`, `.kiro/handoff/README.md`, and the complete spec before assigning work.

Rules:

1. Run Prompt 00 and establish a clean passing baseline.
2. Give each worker one numbered prompt and an isolated branch/worktree.
3. Prevent concurrent ownership of root `Cargo.toml`, `Cargo.lock`, `.github/workflows/ci.yml`, and `tasks.md`; workers may propose changes, but merge them serially.
4. Require each worker to report changed files, tests, unresolved risks, and commit hash.
5. Review security-boundary changes manually. Do not accept string-prefix URL validation, unrestricted Lua libraries, native plugin loading, or unbounded plugin output.
6. Merge one focused commit at a time. After every merge run `./Scripts/check-rust.sh` and `git diff --check`.
7. Update `tasks.md` conservatively. A partial implementation stays `[-]`.
8. Keep `HANDOFF.md` current after each milestone.
9. Never push branches, rename the remote repository, create the plugin repository, publish packages, or configure signing credentials without explicit user approval.
10. Stop a workstream when its prerequisite is missing instead of inventing a temporary insecure bypass.

Suggested first wave:

- Worker A: Prompt 01 (`panel-http`)
- Worker B: Prompt 03 (configuration startup)
- Worker C: Prompt 04 analysis and model-to-view conversion, avoiding root dependency edits until coordinated
- Worker D: Prompt 05 platform trait design only

Merge order for the first wave: A, B, D, C, with a full validation after each merge. Adjust only when actual dependency conflicts require it.

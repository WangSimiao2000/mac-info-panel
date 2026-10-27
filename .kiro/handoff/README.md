# KiroCrew Handoff Prompt Index

Use these prompts as scoped tasks. Start with `00-bootstrap-and-audit.md` once. Then execute the implementation prompts in this dependency order:

```text
00 bootstrap/audit
  |
  +--> 01 panel-http ---------> 02 plugin hardening ---> 06 Lua-in-WASM
  |                                      |
  |                                      +-------------> 07 package CLI
  |
  +--> 03 config bootstrap
  |
  +--> 04 Slint renderer
  |
  +--> 05 platform traits and metrics

01 + 02 + 06 + 07 ----> 08 MC Ops plugin
```

Prompts 01, 03, 04, and the trait-only portion of 05 can be investigated independently, but agents will all touch `Cargo.toml`, `Cargo.lock`, and `tasks.md`. Use separate branches/worktrees and merge one validated commit at a time. Do not let parallel agents overwrite those shared files.

Recommended ownership:

- security agent: 01, 02, 06, 07
- application agent: 03 and `panel-core` integration
- UI agent: 04
- platform agent: 05 and later OS adapters
- coordinator/reviewer: 00, merges, task status, cross-crate validation

Every implementation prompt requires a focused commit. The coordinator should run `./Scripts/check-rust.sh` after each merge and correct `HANDOFF.md` whenever repository state or architectural decisions change.

Do not run Prompt 08 until the user has approved creation of a separate remote repository.

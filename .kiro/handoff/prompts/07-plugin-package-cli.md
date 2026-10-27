# Prompt 07: Plugin package and CLI

Implement section 6 after manifest runtime selection is stable.

Define a deterministic `.info-panel-plugin` ZIP format and implement `apps/plugin-cli` commands:

```text
new
validate
test
pack
```

Reject path traversal, symlinks, duplicate normalized paths, case-colliding paths, oversized entries, oversized total archives, unsupported compression, extra executable payloads, and manifest/entry mismatches. Reproducible packing must normalize ordering, timestamps, and permissions.

Hash the complete deterministic package with SHA-256. Keep signature verification behind a provider interface so repository trust roots can be added later. Development mode may load an unsigned local package only through an explicit flag and must surface a persistent warning.

Add archive attack fixtures and reproducibility tests. Do not create or publish a remote plugin repository in this task. Run complete validation and create one focused commit.

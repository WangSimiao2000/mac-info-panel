# Prompt 01: Secure host HTTP client

Implement section 4 of `.kiro/specs/rust-cross-platform-rewrite/tasks.md` in a new `crates/panel-http` crate.

Required behavior:

- HTTPS by default; HTTP only for structurally verified loopback destinations.
- Reject URL credentials and unsupported schemes.
- Separate connect/request/resource timeouts.
- Stream responses and stop before exceeding the configured byte limit.
- Disable cookies and redirects by default.
- If redirects are later enabled, validate every hop before following it.
- Resolve destinations and reject public-name rebinding to loopback, link-local, private, multicast, or unspecified addresses unless the declared permission explicitly allows that address class.
- Enforce exact manifest origin permissions, concurrency limits, and per-plugin request-rate limits.
- Return typed errors without leaking credentials.

Keep all IO injectable so tests use a local fake server/resolver and do not depend on the public internet. Add tests for IPv4, IPv6, fake localhost domains, credentials, redirect escape, oversized streaming responses, timeout, DNS rebinding, and rate limits.

Do not expose this client to plugins directly yet. Update `tasks.md` only for behavior actually completed. Run `./Scripts/check-rust.sh`, `git diff --check`, and create one focused commit.

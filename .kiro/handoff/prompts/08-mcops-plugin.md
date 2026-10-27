# Prompt 08: MC Ops plugin extraction

Do not begin until `panel-http`, Lua/WASM runtime selection, and plugin packaging are complete.

Design MC Ops as an independent plugin that requests status only through the host HTTP API and returns semantic `PageModel` widgets. It must not execute SSH, shell commands, or native code on the client.

The historical collector is available on `legacy/swift` under `server/` for reference. Do not copy it back into `main`. If a server-side collector remains necessary, place it in the future MC Ops plugin repository, enforce response/body and protocol-length limits, and retain the forced-command/least-privilege model.

Prepare local repository contents, tests, manifest, configuration schema, README, license, and release workflow. Before creating a GitHub repository, pushing code, or publishing a package, stop and ask the user for explicit approval and the desired repository owner/name.

Verify offline fixtures, timeout/error states, malformed server payloads, and maximum player/list sizes. Use the host renderer; do not ship arbitrary UI.

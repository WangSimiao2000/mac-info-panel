# Info Panel plugin package and CLI

`plugin-cli` creates, validates, tests, and packs Info Panel plugins. It never creates a remote repository and never executes plugin entries as native code.

## Deterministic package format

A `.info-panel-plugin` file is a strict ZIP32 archive. Canonical packages use:

- Portable ASCII path segments limited to `[A-Za-z0-9._-]+`, with `/` separators, bytewise lexical entry ordering, and no directory entries. Empty/dot segments, trailing dots/spaces, Windows device names, case collisions, and file/descendant prefix conflicts are forbidden.
- Raw DEFLATE at level 9 for every file.
- DOS timestamp `1980-01-01 00:00:00`.
- Unix regular-file mode `0644`, ZIP version 2.0, no comments, no extra fields, and no data descriptors.
- Required files `manifest.json`, the declared `plugin.wasm` or `plugin.lua`, `README.md`, and `LICENSE`.
- Optional `config.schema.json` and data/media files below `assets/`. Assets are limited to JSON, UTF-8 text/Markdown/CSV, PNG, JPEG, GIF, WebP, BMP, ICO, and AVIF with matching content signatures.

SHA-256 covers every byte of the finished ZIP, including local headers and the central directory. Packing the same file names and contents produces identical bytes.

The parser accepts only this canonical subset. Before decompression it rejects multi-disk and ZIP64 forms, encryption, data descriptors, unsupported compression, malformed or mismatched headers, overlapping/hidden data, non-canonical metadata, links and special-file modes, absolute or ambiguous paths, NULs, backslashes, duplicate paths, case collisions, excessive counts and declared sizes, and compression ratios above the limit. After bounded decompression it checks actual size, CRC-32, package allowlists, executable payloads, JSON, manifest/runtime/entry consistency, and entry type.

Default limits are:

| Boundary | Limit |
| --- | ---: |
| Package bytes | 32 MiB |
| Configuration bytes | 1 MiB |
| Lua runner bytes | 32 MiB |
| Signature bytes | 64 KiB |
| Entries | 256 |
| One uncompressed entry | 16 MiB |
| Total uncompressed bytes | 32 MiB |
| `manifest.json` | 64 KiB |
| `config.schema.json` | 1 MiB |
| Path | 512 bytes |
| Compression ratio | 100:1 |

ZIP has no hard-link entry type. The accepted archive subset contains only independent regular-file byte streams and rejects all extra fields, so link semantics cannot enter a package. Packing opens the source root as a directory capability, enumerates from directory handles, opens each child with no-follow semantics, and validates type, file ID, device/volume, and link count after opening. Unix uses descriptor-relative opens; Windows opens reparse points without following them and checks the handle's 128-bit file ID, reparse attributes, type, and `NumberOfLinks == 1`. Symlinks, hardlinks, junctions, nested mount points, FIFOs, devices, and sockets are rejected.

Before source contents are read, `pack` binds the output parent directory, rejects outputs inside the source tree and any pre-existing output, and proves separation by walking source/output directory-handle ancestry. An incomplete or permission-denied ancestry walk fails closed. The writer creates and `sync_all`s a private temporary file, atomically publishes it with a same-directory no-clobber hard link, removes the temporary name, and synchronizes the parent directory. A target created after preparation is never overwritten.

Package, configuration, signature, entry override, and Lua runner inputs are opened no-follow as regular single-link files and read through independent hard byte limits. File ID, length, type, reparse state, and link count must remain unchanged, and two bounded reads through the same handle must produce identical bytes.

## Trust and development mode

Signature checks use `SignatureProvider`. `NoTrustRoots`, the current default provider, always fails closed. A package cannot pass `validate` or `test` unsigned unless the caller explicitly supplies `--allow-unsigned-local`. That development-only path emits one JSON warning with stable code `plugin.local-unsigned`, the handle-checked canonical local path, full package SHA-256, plugin ID, and plugin version.

A Lua runner independently requires a provider-verified signature, an explicitly trusted SHA-256 digest, or `--allow-local-runner`. The last mode emits `plugin.local-runner` with the same path/hash/plugin provenance. An arbitrary `--lua-runner` path is never trusted by itself.

`test` defaults to an empty permission grant. Every manifest permission must be supplied through the corresponding `--grant-*` option before host loading succeeds. It first validates the package, stages only validated files in a new temporary directory (`0700` with `0600` files on Unix), and then calls `metadata`, `validate_config`, and `refresh` through `PluginHost` with `HostLimits` and `max_host_calls = 0`. WASM uses the restricted Extism adapter. Lua requires an explicitly verified `lua-runner.wasm` and never runs through a native Lua VM. Neither path starts a process or exposes arbitrary network access.

When `config.schema.json` is present, `--config` is bounded and parsed before plugin execution. Schema preflight allows at most 4,096 JSON nodes, depth 64, 128 regular expressions, and 256 local references. External/file `$ref` and `$dynamicRef`, unresolved pointers, recursive references, and cyclic local-reference graphs are rejected. The validator runs in offline mode with format callbacks disabled, and patterns use the non-backtracking regex engine with 64 KiB compile/DFA limits. The plugin's own `validate_config` remains the final semantic check.

## Commands

```text
plugin-cli new <directory> --id <id> --name <name> --source <https-url> [--runtime lua|wasm]
plugin-cli validate <package.info-panel-plugin> (--signature <file> | --allow-unsigned-local)
plugin-cli test <package.info-panel-plugin> (--signature <file> | --allow-unsigned-local) [--config <file>] [--grant-network <origin>] [--grant-cache] [--grant-config] [--grant-locale] [--grant-platform] [--lua-runner <wasm> (--runner-signature <file> | --runner-sha256 <hex> | --allow-local-runner)]
plugin-cli pack <directory> <output.info-panel-plugin> [--entry-file <path>]
```

A generated Lua scaffold is directly packable. A generated WASM scaffold must first be built for `wasm32-unknown-unknown`; pass the resulting module with `pack --entry-file <path>`.

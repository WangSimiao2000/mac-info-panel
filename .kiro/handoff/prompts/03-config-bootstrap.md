# Prompt 03: Connect configuration to startup

Connect `panel-config` to `apps/desktop` and `panel-core` without adding old Mac Info Panel compatibility.

Define platform-specific new configuration locations for Info Panel. On first run, create the new schemaVersion 1 default configuration atomically. On subsequent runs, load JSONC, report field-level diagnostics, and refuse invalid configuration without overwriting it.

Convert validated page configuration into `PageDescriptor` and initialize `AppState`. Define explicit built-in page identifiers instead of relying on unchecked string conventions. Add a filesystem abstraction or temporary-directory tests for first-run creation, existing valid configuration, invalid configuration, atomic-write failure, and concurrent startup behavior.

Do not read or inspect `~/.config/mac-info-panel`. Do not add a migration command. Do not silently replace invalid configuration with defaults.

Update `tasks.md`, run all checks, and create one focused commit.

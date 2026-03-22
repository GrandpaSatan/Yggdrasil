# Yggdrasil Naming Conventions

## Crates / Modules

- **Service crates:** Norse names, lowercase, no prefix. Examples: `munin`, `odin`, `huginn`, `muninn`.
- **Library crates:** Prefixed with `ygg-`. Examples: `ygg-domain`, `ygg-store`, `ygg-embed`, `ygg-mcp`, `ygg-ha`.
- **Internal modules:** Rust `snake_case`. Examples: `handlers`, `state`, `lsh`, `error`.

## Binaries

- Binary name matches the crate name exactly. Example: crate `munin` produces binary `munin`.

## API Endpoints

- Base path: `/api/v1/`
- Resource names: lowercase, plural where representing collections. Examples: `/api/v1/config`, `/api/v1/sync`, `/api/v1/memory`, `/api/v1/merge`.
- Health check: `/health` (no versioned prefix).

## Environment Variables

- Prefixed with the service name in uppercase. Format: `<SERVICE>_<SETTING>`.
- Examples: `MUNIN_DATABASE_URL`, `MUNIN_SYNC_INTERVAL`.

## Configuration Keys (YAML)

- `snake_case` throughout. Nested objects for logical grouping.
- Examples: `listen_addr`, `database_url`, `sync.rsync_path`, `memory.local_path`, `merge.strategy`.
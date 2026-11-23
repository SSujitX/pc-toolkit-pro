# Deep Cleanup rule catalog

These TOML files are embedded into `pctoolkit-core` at build time (`include_str!`)
and validated on load. One file per group; every rule in a file must declare that
file's group:

| File               | `group`       |
| ------------------ | ------------- |
| `system.toml`      | `system`      |
| `application.toml` | `application` |
| `browser.toml`     | `browser`     |
| `development.toml` | `development` |
| `ai.toml`          | `ai`          |
| `special.toml`     | per kind (special kinds only) |

Project build artifacts (`projects`) and AI model stores come from Rust providers
(`cleaner_deep::provider`), not from TOML.

Paths must be original definitions of public, rebuildable cache locations. Never
copy rule files from other (GPL or otherwise) cleaners.

## Gate

```sh
cargo test --manifest-path src-tauri/Cargo.toml -p pctoolkit-core cleaner_deep::catalog
```

`embedded_catalog_is_valid` fails with a list of `{ rule, code, detail }` issues
when a rule breaks the schema or the safety policy. `locale_keys_exist_in_english_resources`
fails when `name_key` / `detail_key` are missing from `src/locales/en.json`.

## Rule fields

| Field                   | Type / values                              | Default    | Notes |
| ----------------------- | ------------------------------------------ | ---------- | ----- |
| `id`                    | `group.camelName`                          | required   | Unique, stable (persisted in history, sent by the UI). |
| `group`                 | `system` `application` `browser` `development` `projects` `ai` | required | Must match the file. |
| `name_key`, `detail_key`| `deepCleaner.rules.*`                      | required   | Add both to every locale. |
| `risk`                  | `safe` \| `recoverable`                    | required   | `recoverable` = re-downloaded / regenerated. |
| `recommended`           | bool                                       | `false`    | Selected by Smart selection when data is found. |
| `requires_elevation`    | bool                                       | `false`    | Execute reports `skipped(needsAdmin)` when not elevated. |
| `processes`             | `["app.exe", ...]`                         | `[]`       | Running → scan `requiresClose`, execute `blocked(runningProcesses)`. Must also be in the platform close allow list (`pctoolkit-platform/src/process.rs`) to be probed/closed. |
| `kind`                  | `files` \| `recycle_bin` \| `disk_cleanup_*` \| `docker_build_cache` \| `conda_caches` \| `rust_toolchains` | `files` | Special kinds take no roots/matcher; the gate checks each kind's group, `requires_elevation`, and whether it may be `recommended`. |
| `mode`                  | `contents` \| `whole_root`                 | `contents` | `contents` never removes the root. `whole_root` also removes the emptied root; only for static cache/log/temp roots under `local`/`roaming`/`home` with an empty matcher. |
| `allow_personal_folder` | bool                                       | `false`    | Only for `home/Downloads`; requires `extensions` + `older_than_days` + `max_depth`, not recommended. |
| `roots`                 | array of root tables (1–32)                | required   | See below. |
| `matcher`               | matcher table                              | match all  | See below. |

### Roots

```toml
{ base = "local", path = "Vendor/App", children = { names = ["Default"], prefixes = ["Profile "] }, suffixes = ["Cache", "Code Cache"] }
```

| Field      | Meaning |
| ---------- | ------- |
| `base`     | `local` (%LOCALAPPDATA%), `roaming` (%APPDATA%), `home` (%USERPROFILE%), `temp` (%TEMP%), `program_data`, `windir`, `system_drive`. |
| `path`     | Relative, `/`-separated. No `..`, `.`, `\`, drive letters, or `%VAR%`. A segment may hold `*` / `?` globs (needs a literal character, not directly under the base) that expand over real child folders. |
| `env`      | `[{ var = "CARGO_HOME" }, { var = "GOPATH", path = "pkg/mod" }]` — first set, absolute variable replaces `base` + `path`; value `off` disables the root. |
| `children` | Expand direct child folders: `names` (exact), `prefixes`, `contains`, or `all = true` (not directly under a base). Case-insensitive. Link-like children are skipped. |
| `suffixes` | Relative paths appended after `children` / `path`; one root per suffix. |

`windir` and `system_drive` roots must be in the explicit allow lists in
`cleaner_deep/safety.rs` (`WINDIR_ALLOWLIST`, `SYSTEM_DRIVE_ALLOWLIST`).

### Matcher

Flat fields are AND-ed; values inside a list are OR-ed. Empty matcher = every file.

| Field             | Meaning |
| ----------------- | ------- |
| `name_contains`   | File name contains any value. |
| `name_glob`       | File name matches any glob (`*`, `?`). |
| `extensions`      | Extension (no dot) is any value. |
| `path_segments`   | A folder between the root and the file has any of these names. |
| `older_than_days` | Modified at least N (≥ 1) days ago. |
| `max_depth`       | Top level only. `0` = files directly in the root. |
| `any_of`, `all_of`, `not` | Nested matchers. |

## Safety (enforced at load, before measuring, and before deleting)

- Never follow or delete link-like entries: symlinks, junctions, any reparse point,
  cloud placeholders (`OFFLINE`, `RECALL_ON_OPEN`, `RECALL_ON_DATA_ACCESS`).
- Roots are refused when: not absolute, contain a link-like component, not a real
  directory, a volume root, `%WINDIR%` (outside the allow list), Program Files
  (both views), the profile root or an ancestor of it, a base folder itself
  (`AppData`, `Local`, `Roaming`, `ProgramData`), a personal folder (Desktop,
  Documents, Downloads, Pictures, Videos, Music, OneDrive*, Saved Games, ...), a
  protected home prefix (`.ssh`, `.aws`, `.config`, `source`, `repos`, `Dropbox`,
  ...), or any `.git` / `.hg` / `.svn` / `.bzr` path.
- Match-all roots may not contain sensitive segments (`cookies`, `credentials`,
  `sessions`, `messages`, `mail`, `accounts`, `models`, `projects`, ...).
- Overlapping roots: a folder that is another rule's root belongs to that (more
  specific) rule and is skipped by the outer rule, even when the inner rule is not
  selected.
- Every file is re-checked (not link-like, still a file, still matching) right
  before deletion. Locked files count as busy failures; the run continues.
- Sizes are logical file lengths; hardlinks are not deduplicated.

## Example

```toml
[[rule]]
id = "app.exampleCache"
group = "application"
name_key = "deepCleaner.rules.exampleCache"
detail_key = "deepCleaner.rules.exampleCacheDetail"
risk = "safe"
recommended = true
processes = ["Example.exe"]
roots = [
  { base = "roaming", path = "Example", suffixes = ["Cache", "Code Cache", "GPUCache"] },
  { base = "local", path = "Example/User Data", children = { names = ["Default"], prefixes = ["Profile "] }, suffixes = ["Cache"] },
]
matcher = { not = { name_glob = ["*.lock"] } }
```

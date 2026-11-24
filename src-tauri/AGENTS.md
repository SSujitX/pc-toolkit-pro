# Rust and Tauri Guidelines

This file applies only to `src-tauri/` and inherits the repository-wide rules in [`../AGENTS.md`](../AGENTS.md).

## Workspace boundaries

- `crates/pctoolkit-core`: platform-neutral product domains, use cases, safety policy, and persistence. It must not depend on Tauri or a WebView.
- `crates/pctoolkit-platform`: OS contracts and Windows implementations. It reports typed capabilities, samples, and safe fallbacks; it does not decide product workflows (no auto-clean policy, no history orchestration, no UI strings).
- `src/` (under `src-tauri`): thin Tauri adapter. Commands validate transport input, call Core, translate typed errors, and publish events.
- Plugins (`tauri-plugin-*`): isolated integration only (opener, process, single-instance, updater, tray via Tauri features). Register plugin init, capabilities, and frontend bindings in the same change. Tray icon itself is bootstrapped in `lib.rs` (`pctoolkit-main-tray`); frontend sets the menu.

Core modules today: `care` (PC Care engine + categories), `cleaner`, `cleaner_deep` (`scan_deep_cleanup_groups` = catalog-only scan of chosen groups, used by Care Junk), `uninstaller` (inventory, installer-kind + quiet command, pure `matching` safety policy, leftover scanners, clean with registry backup) (Deep Cleanup Rust rule catalog/scan/execute — expand with original Windows cache paths only; never import GPL TOML from inspiration trees), `memory`, `power`, `monitor`, `system_info`, `history`, `shared`. Platform modules include `memory`, `monitor`, `power`, `system_info`, `recycle`, `launch`, `process` (probe/close allowlisted images for Deep Cleanup), `gpu`, `registry` (typed root/view key paths, read/enumerate/delete, `reg export` backup), `installed_apps` (Uninstall entries, Store packages via WinRT `PackageManager`), `uninstall_runner` (command split, `ShellExecuteEx` + Job Object wait), `file_ops` (Recycle Bin, delete-on-reboot, `.lnk` resolve, Desktop known folder), `system_entries` (services, scheduled tasks via Task Scheduler COM with `schtasks` fallback, restore points), and related helpers. Adapter commands also include `settings::open_app_data_folder` (shell helper, not a new Core domain). Deep Cleanup commands: `scan_deep_cleanup`, `list_deep_cleanup_drives_command` / `set_deep_cleanup_drives_command` (`cleaner_deep::scope`: fixed drives with label/space, extra project drives persisted in `%LOCALAPPDATA%\PC Toolkit Pro\deep-cleanup-scope.json`, unknown letters → `invalidInput`, system drive ignored; the shared project discovery is invalidated when the scope changes and gets larger limits per extra drive), `execute_deep_cleanup_command`, `probe_running_processes_command`, `close_running_processes_command`, progress event `deep-cleanup-progress`; cancel shares `cancel_cleanup`. Catalog waves: Wave 1 = GPU/browser/app/dev caches + stale downloads; Wave 2 = Update download + Windows.old (honest elevation), Android/editor caches, Docker Desktop UI cache; Wave 3 = project artifacts / Docker prune / Conda / rustup / Disk Cleanup handlers / AI (see `docs/superpowers/specs/2026-09-03-deep-cleaner-catalog-parity-design.md`).

Do not create a giant `ToolkitService` that owns every domain. Keep cleaner, memory, power, and history as separate implementations that collaborate through typed requests/results.

## Rust organization and naming

- Files and modules use `snake_case`; types use precise domain nouns; functions use verbs that state observable behavior.
- Avoid `utils` for domain behavior. Put a helper beside its owner or in a narrowly named shared/infrastructure module (`shared` for cross-domain error types only).
- Keep visibility minimal. A new `pub` or `pub(crate)` API must represent a stable collaboration boundary, not an expedient way around module ownership.
- Prefer small typed request/result structures over long parameter lists and unrelated tuples.
- Prefer `deny_unknown_fields` + camelCase serde for frontend payloads unless an existing protocol already differs.
- Source comments, logs, errors, tests, and assertions must be clear and consistent. Explain safety assumptions, elevation requirements, performance tradeoffs, and fallback reasons.

## Errors, logs, and protocols

- Domain and platform code return typed errors or stable error codes (`CoreError` / `PlatformError`). Convert to Tauri transport errors only in the adapter (`commands/error` and friends).
- Logs use stable domain/event/field names. Log operation IDs, counts, timings, area skip reasons, and error digests — not private full paths or file contents.
- History retention: `get_history_settings` / `update_history_settings` (Core clamps `maxRecords` to 1–1000 and trims `history.json` at once; every append trims to the same limit).
- Persisted structures (Memory Cleaner settings, history) require a clear location under `%LOCALAPPDATA%\PC Toolkit Pro\` and a documented read / default / migrate / reject policy.
- Keep command names and event payloads versionable. Do not retain permanent old/new aliases after a migration window.

## Cleaner safety

- Default cleaner scan must work **without admin**. Denied paths skip-and-continue; do not fail the whole scan for one inaccessible folder.
- Deep Cleanup (`cleaner_deep`): rules are embedded TOML under `crates/pctoolkit-core/rules/deep_cleanup/` (schema in its README), validated at load and by the gate tests `cleaner_deep::catalog` (`embedded_catalog_is_valid`, locale keys). Scan statuses: `found` / `clean` / `notApplicable` / `limited` / `requiresClose` / `unmeasured`, plus a typed `reason` (`needsAdmin`, `toolUnavailable`, `daemonUnavailable`, `handlerMissing`, `timedOut`, `failed`, `discoveryLimited`). Special kinds (`special.rs`, declared in `special.toml`): Disk Cleanup handlers (`IEmptyVolumeCache` on a fresh STA thread, estimates time-boxed ~3 s / 20 s for servicing + Windows.old), Docker build cache, Conda, rustup toolchains (pinned by discovered project roots stay protected); the gate enforces each kind's group, `requires_elevation`, and whether it may be recommended. Windows.old is removed only by the Previous Installations handler. Providers (`provider.rs` → `registered_providers`): `projects` (bounded discovery of home + fixed-drive dev roots, marker-confirmed artifacts, `.gitignore`/embedded-`.git` guard, 7-day active guard, re-confirm at execute, `clean_directory` whole-root) and `ai_provider` (`cleaner_ai_models`, fingerprint re-check). Execute resolves ids against Core-held scan state only; Smart never preselects admin-only items.
- Destructive flows preserve preview/scan results, explicit user intent, progress, cancel, and honest result counts.
- Empty recycle bin and similar privileged/special operations must report failure clearly rather than pretending success. Recycle Bin **size** is `SHQueryRecycleBinW` (Explorer, current user). Emptying is `SHEmptyRecycleBinW`. Do **not** walk `X:\\$Recycle.Bin` (overcounts other SIDs and follows junctions).
- Missing permission, locked files, and platform uncertainty must fail closed or skip with typed reasons.

- Hardening rules (review 2026-11): `is_user_admin` = token elevation (never `net session`); external tools run through `hidden_child` (Job Object kill-on-close for timeout / cancel, bounded reader wait) with absolute System32 paths via `system32_exe` / `powershell_exe` (PATH only for docker, conda, rustup, winget, nvidia-smi); `recycle_path` never permanently deletes — if an item would bypass the Recycle Bin it fails with `wouldDeletePermanently`; uninstaller paths are normalized (`matching::normalize_candidate`: 8.3 expansion, reject `..`, trailing dot/space, ADS, `\\?\` prefixes) before every protection check at scan and clean time; leftover removal never follows a link (only the link itself is removed); `OtherPrograms` includes hidden / child / update uninstall entries; busy flags are RAII guards (panic-safe).

## App startup settings

- Core `app_settings` (persistence + pure `plan_registration`), platform `autostart` (HKCU Run value, elevated logon task via `startup_items` task helpers), adapter `commands/settings.rs` (`get_app_startup_settings`, `set_app_startup_settings`, `apply_startup_registration`). `run()` reads settings before building Tauri: run-as-admin relaunch via `relaunch_self_elevated_with_args` then `exit(0)` (no single-instance lock yet); `StartHidden` state makes the first `show_main_window` a no-op for `--autostart` + start minimized; setup thread reconciles the registration.

## PC Care

- Core `care` (no Tauri): `CareCategory` trait + types in `care/mod.rs`, categories in `care/categories/<id>.rs` (`all()` = display order), orchestration in `care/engine.rs` (`scan_care`, `fix_care`, `stop_care`, `skip_care_category`, `last_care_scan`). Own busy flag (separate from cleaner/uninstaller); each category gets its own skip flag plus the shared stop flag through `CareContext::should_stop()`.
- Scan: selected categories run on a pool of min(cpus, 4) threads, longest-expected first; panics / errors become `failed`, `OperationBusy` becomes `skipped(busy)`, stop/skip → `skipped`. Progress = full snapshot (`CareProgress`) throttled to 50 ms with forced emits on state changes; percent is weighted by learned per-category durations (`care-timings.json`, EMA α 0.3, version 1, unreadable → `default_weight_ms`), running categories interpolate elapsed/expected capped at 95 %.
- Fix: requests carry `{ items: [{ category, itemIds }] } | { auto }` + `afterAction`; `plan_fix` routes ids against the Core-held last scan (`unknownItem` / `notFixable` / `needsAdmin` rejected up front), categories run sequentially in display order and must re-validate. Auto fix = `risk == safe && fixable && (!needsAdmin || admin)`. After-action (`shutdown` / `restart` / `sleep` / `hibernate`) runs through `power::execute_power` only after all fixes and only when not stopped. One history record per scan and per fix, counts only.
- Junk (`categories/junk.rs`) = `cleaner_deep::scan_deep_cleanup_groups` (catalog rules of system / application / browser, no providers) filtered to `recommended && found`, never `system.recycleBin` / Downloads rules; fix = `execute_deep_cleanup` by rule id, waits up to 30 s on the Deep Cleanup busy flag, mirrors Care stop into `cancel_cleanup`.
- Fix after-action: Auto fix only; shutdown / restart are scheduled with a 60 s grace (`shutdown /s|/r /t 60`, cancellable via `cancel_care_after_action` → `shutdown /a`), never instant; sleep / hibernate run immediately.
- Undo / backups: setting and registry changes write `care-undo.json` (versioned, 1000 records, corrupt → renamed `.corrupt`, record written before the change, dropped if it fails; restore skips items changed since the fix) and `.reg` exports under `care-backups\<timestamp>\` (no backup → no delete). Commands `list_care_undo_records`, `undo_care_fixes(recordIds)`.
- Category rules: Security = data-driven check table (`care/security_checks.rs`), only checks that can be read and reverted; Protection = Security Center + `INetFwPolicy2` (never `netsh firewall`, never toggles Defender); Windows Update = read-only, offline WUA search capped at 20 s; Registry = orphaned uninstall / App Paths / SharedDLLs only (never MRUs, COM, `HKLM\SYSTEM`); Privacy = MRUs / jump lists / browser history+cookies (Review, browser closed, never passwords / site storage); Shortcuts = only missing targets on mounted local fixed drives (`shortcut_batch` keeps one COM session); Software = `winget upgrade` table parse, one upgrade at a time; Disk health = read-only (dirty bit needs admin; repair only `chkdsk /scan` + `chkntfs /c` for a dirty volume); Disk optimize = `defrag X: /O`, never disables the scheduled optimization task.
- Adapter `commands/care.rs`: `scan_care(categories)`, `fix_care(request)`, `stop_care`, `skip_care_category(category)`, `open_care_action(action)` (allowlist `care::CARE_ACTIONS` → `ms-settings:` / `windowsdefender:` URIs via the opener plugin's Rust API); event `care-progress`.
- Startup (`startup.rs` domain + `care/categories/startup.rs`): platform `startup_items` reads facts (Run values per view, Startup folder `.lnk` targets, Task Scheduler COM tasks with enabled logon/boot triggers, Win32 services — never drivers, packaged startup tasks from each non-framework package's `AppxManifest.xml` (`windows.startupTask` matched by local name, executable from the extension else its `Application`, `ms-resource:` names via `SHLoadIndirectString`, label = package display name) + per-user `SystemAppData\<family>\<taskId>\State`, `StartupApproved` bytes: odd first byte = disabled + FILETIME, file version CompanyName, `EvtQuery` on `Microsoft-Windows-Diagnostics-Performance/Operational`) and performs switches (`StartupApproved` write, `IRegisteredTask::put_Enabled`, `ChangeServiceConfig[2]W`, `RegisterTask` non-elevated logon task, packaged `State` = 2 / 1, refused with `policyLocked` for 3 / 4). Core owns policy: ids (`run:<cu|lm>:<64|32>:<value>`, `folder:<cu|lm>:<file>`, `task:<path>`, `svc:<name>`, `pkg:<family>:<taskId>`), Microsoft tasks/services read-only (Microsoft packaged tasks stay toggleable, like Settings), packaged tasks: publisher + logo icon from the package, never broken / delayed / deleted, no admin, delay for per-user Run/Startup-folder entries + services only, broken = missing file on a present local drive, impact thresholds (≥1 s high, ≥300 ms medium), undo/known/backup files. Actions re-read the item first and skip admin-only changes when not elevated. Commands: `list_startup_items_command`, `startup_item_action_command` (`enable|disable|delay|undelay|delete`), `get_startup_icons_command`, `reveal_startup_item_command` (opener Rust API; path resolved in Core). Care reports enabled third-party Run / Startup folder / task / packaged entries as Review (never auto-fixed, services excluded) and broken entries as Safe (delete with backup). Platform tests touch the real registry only under `HKCU\Software\PC Toolkit Pro Test`.

## Uninstaller

- Commands: `list_installed_programs`, `uninstall_program_command`, `scan_program_leftovers_command`, `preview_program_related_data` (read-only data-folder preview: `leftovers::scan_related_data` = named app-data folders + profile dot-folders, install folder excluded; no busy lock; never stored in `SCANS`), `clean_uninstall_leftovers`, `get_program_icons_command`, `focus_uninstaller_window`, `dismiss_removed_program_command`, `cancel_uninstaller`; progress event `uninstaller-progress` (listing emits phase `listing` with `area` = `registry` | `desktop` | `store` | `done`, the program being read with its `itemId` + cached `icon` data URL, and counts; per-program events throttled to ~30 ms, so only programs actually shown pay for icon extraction). Own busy/cancel flags (separate from cleaner). Core caches the last inventory and scans; clean requests carry `scanId` + item ids only.
- `uninstaller/matching.rs` is the safety boundary (exact normalized names, generic-token list, protected roots, shared-folder guard, absolute drive paths only) and must keep unit tests for every rule change. Execution re-checks the protected policy.
- Registry deletes require a successful `reg export` backup under `%LOCALAPPDATA%\PC Toolkit Pro\uninstaller-backups\<timestamp>\`; services/tasks/HKLM/Program Files items report `skippedNeedAdmin` when not elevated. Store removal uses `PackageManager.RemovePackageAsync` for the current user only.
- Icons: `DisplayIcon` resource → MSI `ProductIcon` (packed GUID) → main exe in install dir → uninstaller exe, extracted with `PrivateExtractIconsW` / `SHGetFileInfoW` and encoded by the in-crate PNG writer (`app_icons`); Store logos read from the package's best scale variant. Cached per program id; never blocks the busy flag.
- Progress carries `currentPath`, `windowVisible` (a visible window belongs to the uninstaller Job tree), and per-item `itemStatus` while cleaning.
- `uninstaller/snapshot.rs` persists `uninstaller-inventory.json` (versioned; parse error → empty baseline) and reports `removedElsewhere` in `ProgramInventory`; `find_record` falls back to a snapshot record so leftovers of an already-removed program can be scanned (uninstall key rebuilt from the `d:<lm|cu>:<64|32>:<leaf>` id). Install date falls back to the uninstall key's last write time.
- `matching::ProtectedRoots` exempts children of the Start Menu `Programs` folders and pinned-taskbar folder from the `AppData\Roaming\Microsoft` / `ProgramData\Microsoft` subtree rule; the scanner and the execution re-check use the same policy.
- Do not add kernel drivers, install-monitor filter drivers, or third-party rule databases.

## Memory cleaner

- Stats and optimize areas use real Win32 / NT APIs in `pctoolkit-platform` (for example `GlobalMemoryStatusEx`, `NtSetSystemInformation` and related calls).
- Core owns settings persistence, auto-interval steps (from **5 minutes** upward), free-RAM threshold / cooldown policy, history writes, and orchestration of `optimize_memory`.
- When elevation is missing, skip or fail **per area** with honest status — never fake success via PowerShell or empty stubs.
- Memory Cleaner UI can call `restart_as_administrator` (`relaunch_self_elevated` / `runas`) then exit so the elevated instance takes the single-instance lock. Do not set the EXE to `requireAdministrator`; keep `asInvoker` in `windows/app.manifest`.
- Commands: `get_elevation_status`, `restart_as_administrator` alongside stats/settings/optimize/cancel.
- Cleaner `freeMemory` (if exposed) must wire through the same memory engine so tray, page, and cleaner stay consistent.
- Auto-clean from tray and UI must share the same settings file and optimize entry point. The scheduler is `commands::memory::start_auto_clean_scheduler` (started in `setup`) → Core `auto_clean_tick` / pure `auto_clean_decision`; it emits `memory-auto-optimized` `{ reason, result }` after each run.
- Modified File Cache = `FlushFileBuffers` on fixed-volume handles opened `GENERIC_READ | GENERIC_WRITE` (admin); all-denied → `skippedNeedAdmin`. Do not send guessed FSCTL codes (the commonly copied `0x90054` is `FSCTL_INVALIDATE_VOLUMES`).

## Power, monitor, system info

- Power actions and scheduled shutdown live in platform + Core; adapter commands stay thin. Prefer `System32\shutdown.exe`, wait for exit status (Python-era `check=True`), and cancel-then-reschedule before a new `/t` timer. Lock/sleep use Win32 (`LockWorkStation` / `SetSuspendState`), not rundll32 stubs.
- Quick-action GUI tools launch via System32 + `cmd /C start` so PATH/console flags from the Tauri host do not block Task Manager and peers.
- Monitor samples feed the titlebar and Monitor page; keep sampling cheap and non-blocking on the UI path. Titlebar/tray RAM % must use `memory_stats()` — physical load from `GetPerformanceInfo` (Task Manager semantics: available = standby + free + zeroed; `GlobalMemoryStatusEx` stays the fallback and the virtual/commit source), not `dwMemoryLoad` or a divergent sysinfo ratio. Reuse one persistent `sysinfo::System` for CPU sampling so polls produce real usage instead of ~0%. Cache `nvidia-smi` (~5s); never spawn it on every 1s titlebar poll (late replies can pin a stale RAM%). Avoid sleeping inside every `sample_monitor` poll.
- System information collection must avoid flashing a console window. Prefer quiet Win32 / WMI-style collection with typed fields (CPU, disks, RAM, GPU, monitors, motherboard, OS, PSU name when available). Emit staged `SystemInfoProgress` (`metrics` → `hardware` → `gpu` → `assemble`) for the Information UI.
- Memory privilege enable must treat `ERROR_NOT_ALL_ASSIGNED` as failure; map `STATUS_PRIVILEGE_NOT_HELD` to `skippedNeedAdmin`, not fake `ok` / vague `failed`.

## Platform code

- Define the contract before moving an implementation. Platform facts must not import cleaner UI concepts or frontend locale strings.
- Keep `cfg` at narrow module or item boundaries. This product is Windows-first; do not pretend full macOS/Linux support without real implementations.
- Native fast paths require a correct fallback and diagnostics that distinguish success, skip, and failure.
- Privileged operations require a separately reviewed capability boundary. Do not expand Tauri permissions “just in case,” and do not simulate a privileged helper inside an ordinary cleaner path.

## Tauri adapter and plugins

- Command handlers remain async adapters and contain no scan, cleanup, memory-optimize, or persistence algorithms.
- Register every command, permission, capability, frontend service binding, and plugin initialization in the same change.
- Capability scopes stay minimal (`capabilities/default.json`). Tray/menu needs `core:tray:default` + `core:menu:default` + `core:image:default`; updater needs `updater:default` only when wired end-to-end. Do not assume `opener:default` includes `allow-open-path`.
- App startup must not perform long scans or blocking filesystem work before the first window is shown. Tray icon bootstrap is allowed at setup (cheap); full menus attach from Vue after mount.
- Window close on `main` hides to tray; Exit from tray quits. Preserve single-instance focus behavior.
- Updater: `createUpdaterArtifacts` + pubkey/endpoints in `tauri.conf.json`; CI signs with `TAURI_SIGNING_PRIVATE_KEY` secrets and publishes `latest.json`. Write the secret to a temp file, then set `TAURI_SIGNING_PRIVATE_KEY` from that file in the build step (avoid `Out-File` UTF-8 BOM into `GITHUB_ENV`). Never commit private keys under `.tauri/`.
- NSIS: `bundle.windows.nsis.installMode` is **`currentUser`** (install under `%LOCALAPPDATA%\PC Toolkit Pro\`, Start Menu + uninstall, no admin for install). `mainBinaryName` is `PCToolkitPro`.
- Release: **Actions → Release → bump** (`current|patch|minor|major`) from root `VERSION`. Publishes **`PC Toolkit Pro v{version} Setup.exe`** / **`PC Toolkit Pro v{version}.exe`**, `SHA256SUMS`, and updater `latest.json` (URL must use the setup filename). Categorized conventional-commit notes. Do not free-type semver.
- `tauri` Cargo features for tray icons include `tray-icon`, `image-png`, and `image-ico` when embedding/loading tray images.

## Blocking work and cancel

- Run blocking Core/platform work only via `spawn_blocking` / `run_blocking` (or equivalent).
- Emit progress events from Core callbacks; never invent UI strings in Rust for control flow.
- Support cancel for long operations; return typed `operationBusy` instead of deadlocking when a second operation starts.

## Validation

For Rust changes, run what the local toolchain allows:

```sh
cargo check --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml -p pctoolkit-core
```

When MSVC/`link.exe` is missing locally, do not claim a full Windows link succeeded — use GitHub Actions (`.github/workflows/tauri-build.yml` / `release-windows.yml`) for real binaries and state the unvalidated local scope.

Changes to cleaner safety, memory optimize areas, persistence, elevation paths, or performance require tests or a reproducible observation appropriate to the affected behavior. Keep raw machine evidence and private datasets outside the repository.

When a change adds a Core/platform module, Tauri command/event/plugin, persistence path, or Win32 honesty/elevation rule, update **this** file (and root [`AGENTS.md`](../AGENTS.md) if cross-cutting) in the same change. See `.cursor/rules/agents-md-sync.mdc`.

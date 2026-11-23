//! Developer tool caches driven through each tool's own CLI (Docker build cache, Conda package
//! caches, rustup toolchains).
//!
//! Every child process runs without a console window, with a timeout and capped output. Output
//! that does not parse fails closed (`invalidOutput`): we never guess sizes or delete on a partial
//! read. Errors carry typed reasons and exit codes only — never raw stderr, which can contain
//! private paths.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::hidden_child::{CappedReader, ChildJob, READER_GRACE};
use crate::process::hide_console;

// ------------------------------------------------------------------------------------------------
// Typed errors

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DevToolReason {
    /// Executable not found on PATH / known install locations.
    ToolUnavailable,
    /// Docker CLI present but the engine is not reachable.
    DaemonUnavailable,
    Timeout,
    InvalidOutput,
    Cancelled,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DevToolError {
    pub reason: DevToolReason,
    pub exit_code: Option<i32>,
}

impl DevToolError {
    fn new(reason: DevToolReason) -> Self {
        Self { reason, exit_code: None }
    }
}

impl std::fmt::Display for DevToolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.exit_code {
            Some(code) => write!(f, "{:?} (exit {code})", self.reason),
            None => write!(f, "{:?}", self.reason),
        }
    }
}

impl std::error::Error for DevToolError {}

pub type DevToolResult<T> = Result<T, DevToolError>;

// ------------------------------------------------------------------------------------------------
// Process runner

const OUTPUT_CAP: usize = 8 * 1024 * 1024;
const POLL: Duration = Duration::from_millis(50);

struct ToolOutput {
    exit_code: Option<i32>,
    success: bool,
    stdout: String,
    stderr: String,
}

fn run_tool(
    program: &Path,
    args: &[&str],
    timeout: Duration,
    should_cancel: &dyn Fn() -> bool,
) -> DevToolResult<ToolOutput> {
    let mut cmd = Command::new(program);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        // Keep tool output stable for parsing.
        .env("NO_COLOR", "1");
    hide_console(&mut cmd);
    let mut child = cmd.spawn().map_err(|error| match error.kind() {
        std::io::ErrorKind::NotFound => DevToolError::new(DevToolReason::ToolUnavailable),
        _ => DevToolError::new(DevToolReason::Failed),
    })?;
    // Kill-on-close job: a timeout / cancel terminates the whole tree, so no grandchild keeps
    // the output pipes open; readers are still joined with a deadline.
    let job = ChildJob::attach(&child);
    let stdout = child.stdout.take().map(|s| CappedReader::spawn(s, OUTPUT_CAP, |_| {}));
    let stderr = child.stderr.take().map(|s| CappedReader::spawn(s, OUTPUT_CAP, |_| {}));

    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) => {}
            Err(_) => break Err(DevToolReason::Failed),
        }
        if should_cancel() {
            break Err(DevToolReason::Cancelled);
        }
        if started.elapsed() >= timeout {
            break Err(DevToolReason::Timeout);
        }
        thread::sleep(POLL);
    };
    if status.is_err() {
        if let Some(job) = &job {
            job.terminate();
        }
        let _ = child.kill();
        let _ = child.wait();
    } else if let Some(job) = job {
        job.release();
    }
    let join = |reader: Option<CappedReader>| {
        reader
            .map(|r| String::from_utf8_lossy(&r.finish(READER_GRACE)).into_owned())
            .unwrap_or_default()
    };
    let stdout = join(stdout);
    let stderr = join(stderr);
    let status = status.map_err(DevToolError::new)?;
    Ok(ToolOutput {
        exit_code: status.code(),
        success: status.success(),
        stdout,
        stderr,
    })
}

fn failed(output: &ToolOutput) -> DevToolError {
    DevToolError {
        reason: DevToolReason::Failed,
        exit_code: output.exit_code,
    }
}

/// Find an executable on PATH by exact file names (in order), without invoking a shell.
fn find_on_path(names: &[&str]) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        for name in names {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
}

/// Directory size without following symlinks / junctions.
fn dir_size(path: &Path) -> u64 {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return 0;
    };
    if is_link_like(&meta) {
        return 0;
    }
    if meta.is_file() {
        return meta.len();
    }
    let mut total = 0u64;
    let mut stack = vec![path.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(meta) = std::fs::symlink_metadata(entry.path()) else {
                continue;
            };
            if is_link_like(&meta) {
                continue;
            }
            if meta.is_dir() {
                stack.push(entry.path());
            } else {
                total = total.saturating_add(meta.len());
            }
        }
    }
    total
}

fn is_link_like(meta: &std::fs::Metadata) -> bool {
    if meta.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        meta.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        false
    }
}

// ------------------------------------------------------------------------------------------------
// Running process probe (shared guard for tool-owned stores, e.g. Ollama models)

/// Number of running processes whose image name matches any of `images` (case-insensitive,
/// `.exe` optional). Read-only; used by Core to refuse deleting a store an app has open.
pub fn count_running_images(images: &[&str]) -> u32 {
    use sysinfo::{ProcessesToUpdate, System};
    let wanted: HashSet<String> = images.iter().map(|i| normalize_image(i)).collect();
    if wanted.is_empty() {
        return 0;
    }
    let mut sys = System::new();
    sys.refresh_processes(ProcessesToUpdate::All, true);
    sys.processes()
        .values()
        .filter(|p| wanted.contains(&normalize_image(&p.name().to_string_lossy())))
        .count() as u32
}

fn normalize_image(name: &str) -> String {
    let lower = name.trim().to_ascii_lowercase();
    if lower.ends_with(".exe") {
        lower
    } else {
        format!("{lower}.exe")
    }
}

// ------------------------------------------------------------------------------------------------
// Sizes

/// Parse a human size as printed by Docker (`go-units`: decimal `kB`/`MB`/`GB`) or binary units
/// (`KiB`/`MiB`/`GiB`). Returns `None` on anything unrecognized so callers fail closed.
fn parse_human_size(text: &str) -> Option<u64> {
    let text = text.trim();
    let split = text
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(text.len());
    let (number, unit) = text.split_at(split);
    let value: f64 = number.parse().ok()?;
    if !value.is_finite() || value < 0.0 {
        return None;
    }
    let multiplier: f64 = match unit.trim() {
        "" | "B" => 1.0,
        "kB" | "KB" | "k" => 1e3,
        "MB" | "M" => 1e6,
        "GB" | "G" => 1e9,
        "TB" | "T" => 1e12,
        "PB" => 1e15,
        "KiB" => 1024.0,
        "MiB" => 1024.0 * 1024.0,
        "GiB" => 1024.0 * 1024.0 * 1024.0,
        "TiB" => 1024.0 * 1024.0 * 1024.0 * 1024.0,
        _ => return None,
    };
    Some((value * multiplier).round() as u64)
}

// ------------------------------------------------------------------------------------------------
// Docker build cache

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DockerBuildCachePreview {
    /// Bytes in build-cache records that are neither in use nor shared.
    pub reclaimable_bytes: u64,
    pub record_count: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DockerBuildCachePrune {
    pub before_bytes: u64,
    pub remaining_bytes: u64,
    pub released_bytes: u64,
}

const DOCKER_PREVIEW_TIMEOUT: Duration = Duration::from_secs(90);
const DOCKER_PRUNE_TIMEOUT: Duration = Duration::from_secs(30 * 60);

fn docker_exe() -> DevToolResult<PathBuf> {
    find_on_path(&["docker.exe", "docker"])
        .or_else(|| {
            let base = std::env::var_os("ProgramFiles")?;
            let path = PathBuf::from(base).join(r"Docker\Docker\resources\bin\docker.exe");
            path.is_file().then_some(path)
        })
        .ok_or(DevToolError::new(DevToolReason::ToolUnavailable))
}

fn docker_failure(output: &ToolOutput) -> DevToolError {
    let stderr = output.stderr.to_ascii_lowercase();
    let daemon_down = [
        "cannot connect to the docker daemon",
        "failed to connect to the docker api",
        "error during connect",
        "is the docker daemon running",
        "docker desktop is not running",
        "pipe/docker_engine",
    ]
    .iter()
    .any(|needle| stderr.contains(needle));
    DevToolError {
        reason: if daemon_down {
            DevToolReason::DaemonUnavailable
        } else {
            DevToolReason::Failed
        },
        exit_code: output.exit_code,
    }
}

fn json_flag(value: Option<&serde_json::Value>) -> Option<bool> {
    match value? {
        serde_json::Value::Bool(b) => Some(*b),
        serde_json::Value::String(s) => match s.trim().to_ascii_lowercase().as_str() {
            "true" => Some(true),
            "false" => Some(false),
            _ => None,
        },
        _ => None,
    }
}

fn json_size(value: Option<&serde_json::Value>) -> Option<u64> {
    match value? {
        serde_json::Value::Number(n) => n.as_u64(),
        serde_json::Value::String(s) => parse_human_size(s),
        _ => None,
    }
}

/// Parse `docker system df -v --format "{{json .}}"`.
///
/// Modern CLIs print one object with a `BuildCache` array (`InUse`, `Shared`, `Size` per record).
/// Older ones print JSON lines per type; for those we accept the `Build Cache` summary line's
/// `Reclaimable` size. Any malformed record fails the whole parse.
fn parse_docker_df(stdout: &str) -> Option<DockerBuildCachePreview> {
    let mut found = false;
    let mut bytes = 0u64;
    let mut records = 0u32;
    for line in stdout.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let value: serde_json::Value = serde_json::from_str(line).ok()?;
        if let Some(cache) = value.get("BuildCache") {
            found = true;
            if cache.is_null() {
                continue;
            }
            for record in cache.as_array()? {
                let in_use = json_flag(record.get("InUse"))?;
                let shared = json_flag(record.get("Shared")).unwrap_or(false);
                let size = json_size(record.get("Size"))?;
                if !in_use && !shared {
                    bytes = bytes.saturating_add(size);
                    records += 1;
                }
            }
        } else if value.get("Type").and_then(|t| t.as_str()) == Some("Build Cache") {
            found = true;
            let reclaimable = value.get("Reclaimable")?.as_str()?;
            // "1.2GB (100%)"
            let size = reclaimable.split_whitespace().next()?;
            bytes = bytes.saturating_add(parse_human_size(size)?);
            records += value
                .get("TotalCount")
                .and_then(|c| c.as_str().and_then(|s| s.parse().ok()).or_else(|| c.as_u64().map(|n| n as u32)))
                .unwrap_or(0);
        }
    }
    found.then_some(DockerBuildCachePreview {
        reclaimable_bytes: bytes,
        record_count: records,
    })
}

fn docker_preview_with(exe: &Path, should_cancel: &dyn Fn() -> bool) -> DevToolResult<DockerBuildCachePreview> {
    let output = run_tool(
        exe,
        &["--context", "default", "system", "df", "-v", "--format", "{{json .}}"],
        DOCKER_PREVIEW_TIMEOUT,
        should_cancel,
    )?;
    if !output.success {
        return Err(docker_failure(&output));
    }
    parse_docker_df(&output.stdout).ok_or(DevToolError::new(DevToolReason::InvalidOutput))
}

/// Read-only: reclaimable Docker build cache on the `default` context.
pub fn docker_build_cache_preview(should_cancel: &dyn Fn() -> bool) -> DevToolResult<DockerBuildCachePreview> {
    docker_preview_with(&docker_exe()?, should_cancel)
}

/// Destructive: `docker builder prune --all --force`, then re-preview to verify.
pub fn docker_build_cache_prune(should_cancel: &dyn Fn() -> bool) -> DevToolResult<DockerBuildCachePrune> {
    let exe = docker_exe()?;
    let before = docker_preview_with(&exe, should_cancel)?.reclaimable_bytes;
    let output = run_tool(
        &exe,
        &["--context", "default", "builder", "prune", "--all", "--force"],
        DOCKER_PRUNE_TIMEOUT,
        should_cancel,
    )?;
    if !output.success {
        return Err(docker_failure(&output));
    }
    let remaining = docker_preview_with(&exe, &|| false)?.reclaimable_bytes;
    Ok(DockerBuildCachePrune {
        before_bytes: before,
        remaining_bytes: remaining,
        released_bytes: before.saturating_sub(remaining),
    })
}

// ------------------------------------------------------------------------------------------------
// Conda

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CondaCleanPreview {
    pub tarball_bytes: u64,
    pub logfile_bytes: u64,
    pub index_cache_bytes: u64,
    pub total_bytes: u64,
}

const CONDA_PREVIEW_TIMEOUT: Duration = Duration::from_secs(120);
const CONDA_EXECUTE_TIMEOUT: Duration = Duration::from_secs(15 * 60);
/// Only these caches. Never `--all` / `--packages` (extracted packages can be hard-linked into envs).
const CONDA_CLEAN_ARGS: [&str; 5] = ["clean", "--index-cache", "--tarballs", "--logfiles", "--json"];

fn conda_exe() -> DevToolResult<PathBuf> {
    std::env::var_os("CONDA_EXE")
        .map(PathBuf::from)
        .filter(|p| p.is_file())
        .or_else(|| find_on_path(&["conda.exe"]))
        .or_else(|| {
            let home = home_dir()?;
            ["miniconda3", "anaconda3", "miniforge3", "mambaforge"]
                .iter()
                .map(|d| home.join(d).join(r"Scripts\conda.exe"))
                .find(|p| p.is_file())
        })
        .ok_or(DevToolError::new(DevToolReason::ToolUnavailable))
}

/// Parse `conda clean ... --json` output. Index cache has no size in conda's report, so the
/// listed cache directories are returned for the caller to measure.
fn parse_conda_clean(stdout: &str) -> Option<(CondaCleanPreview, Vec<PathBuf>)> {
    // Conda may print warnings before the JSON document.
    let start = stdout.find('{')?;
    let end = stdout.rfind('}')?;
    let value: serde_json::Value = serde_json::from_str(stdout.get(start..=end)?).ok()?;
    if value.get("success").and_then(|s| s.as_bool()) == Some(false) {
        return None;
    }
    let size_of = |key: &str| -> Option<u64> {
        match value.get(key) {
            None | Some(serde_json::Value::Null) => Some(0),
            Some(section) => section.get("total_size").map_or(Some(0), |n| n.as_u64()),
        }
    };
    let tarball_bytes = size_of("tarballs")?;
    let logfile_bytes = size_of("logfiles")?;
    let index_dirs = match value.get("index_cache").and_then(|i| i.get("files")) {
        None | Some(serde_json::Value::Null) => Vec::new(),
        Some(files) => files
            .as_array()?
            .iter()
            .map(|f| f.as_str().map(PathBuf::from))
            .collect::<Option<Vec<_>>>()?,
    };
    Some((
        CondaCleanPreview {
            tarball_bytes,
            logfile_bytes,
            index_cache_bytes: 0,
            total_bytes: tarball_bytes.saturating_add(logfile_bytes),
        },
        index_dirs,
    ))
}

fn conda_run(exe: &Path, dry_run: bool, should_cancel: &dyn Fn() -> bool) -> DevToolResult<CondaCleanPreview> {
    let mut args: Vec<&str> = CONDA_CLEAN_ARGS.to_vec();
    args.push(if dry_run { "--dry-run" } else { "--yes" });
    let timeout = if dry_run { CONDA_PREVIEW_TIMEOUT } else { CONDA_EXECUTE_TIMEOUT };
    let output = run_tool(exe, &args, timeout, should_cancel)?;
    if !output.success {
        return Err(failed(&output));
    }
    let (mut preview, index_dirs) =
        parse_conda_clean(&output.stdout).ok_or(DevToolError::new(DevToolReason::InvalidOutput))?;
    if dry_run {
        preview.index_cache_bytes = index_dirs.iter().map(|d| dir_size(d)).sum();
        preview.total_bytes = preview.total_bytes.saturating_add(preview.index_cache_bytes);
    }
    Ok(preview)
}

/// Read-only: what `conda clean --index-cache --tarballs --logfiles` would free.
pub fn conda_clean_preview(should_cancel: &dyn Fn() -> bool) -> DevToolResult<CondaCleanPreview> {
    conda_run(&conda_exe()?, true, should_cancel)
}

/// Destructive: run the same clean with `--yes`, then return a fresh dry-run as the remaining size.
pub fn conda_clean_execute(should_cancel: &dyn Fn() -> bool) -> DevToolResult<CondaCleanPreview> {
    let exe = conda_exe()?;
    conda_run(&exe, false, should_cancel)?;
    conda_run(&exe, true, &|| false)
}

// ------------------------------------------------------------------------------------------------
// Rust toolchains

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RustToolchainFact {
    pub name: String,
    pub is_default: bool,
    pub is_active: bool,
    pub is_override: bool,
    /// Pinned by a `rust-toolchain(.toml)` in a caller-provided project root.
    pub is_pinned: bool,
    pub protected: bool,
    pub bytes: u64,
    #[serde(skip)]
    pub path: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RustToolchainInventory {
    pub toolchains: Vec<RustToolchainFact>,
    pub removable_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RustToolchainUninstallStatus {
    Removed,
    /// Refused: protected at execution time (default / active / override / pinned).
    Protected,
    /// Not installed (any more) when re-listed.
    NotFound,
    /// rustup reported success but the toolchain is still listed.
    StillPresent,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RustToolchainUninstall {
    pub name: String,
    pub status: RustToolchainUninstallStatus,
    pub released_bytes: u64,
}

const RUSTUP_LIST_TIMEOUT: Duration = Duration::from_secs(30);
const RUSTUP_UNINSTALL_TIMEOUT: Duration = Duration::from_secs(10 * 60);

fn rustup_exe() -> DevToolResult<PathBuf> {
    find_on_path(&["rustup.exe", "rustup"])
        .or_else(|| {
            let cargo_home = std::env::var_os("CARGO_HOME")
                .map(PathBuf::from)
                .or_else(|| home_dir().map(|h| h.join(".cargo")))?;
            let path = cargo_home.join("bin").join("rustup.exe");
            path.is_file().then_some(path)
        })
        .ok_or(DevToolError::new(DevToolReason::ToolUnavailable))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ListedToolchain {
    name: String,
    is_default: bool,
    is_active: bool,
    is_override: bool,
    path: Option<PathBuf>,
}

/// Parse `rustup toolchain list -v`. Accepts both rustup ≥1.28 (`name (active, default) path`)
/// and older output (`name (default)\tpath`, `name path`).
fn parse_toolchain_list(stdout: &str) -> Option<Vec<ListedToolchain>> {
    let mut out = Vec::new();
    for line in stdout.lines().map(str::trim).filter(|l| !l.is_empty()) {
        if line.eq_ignore_ascii_case("no installed toolchains") {
            continue;
        }
        let (name, mut rest) = match line.find(char::is_whitespace) {
            Some(i) => (&line[..i], line[i..].trim()),
            None => (line, ""),
        };
        if !is_valid_toolchain_name(name) {
            return None;
        }
        let mut flags = "";
        if rest.starts_with('(') {
            let close = rest.find(')')?;
            flags = &rest[1..close];
            rest = rest[close + 1..].trim();
        }
        let has = |flag: &str| flags.split(',').any(|f| f.trim() == flag);
        out.push(ListedToolchain {
            name: name.to_string(),
            is_default: has("default"),
            is_active: has("active"),
            is_override: has("override"),
            path: (!rest.is_empty()).then(|| PathBuf::from(rest)),
        });
    }
    Some(out)
}

/// Parse `rustup override list`: `<path>\t<toolchain>` per line, or `no overrides`.
fn parse_override_list(stdout: &str) -> Option<Vec<String>> {
    let mut out = Vec::new();
    for line in stdout.lines().map(str::trim).filter(|l| !l.is_empty()) {
        if line.eq_ignore_ascii_case("no overrides") {
            continue;
        }
        let name = line.split_whitespace().last()?;
        if !is_valid_toolchain_name(name) {
            return None;
        }
        out.push(name.to_string());
    }
    Some(out)
}

/// First token of `rustup show active-toolchain` (`stable-x86_64-pc-windows-msvc (default)`).
fn parse_active_toolchain(stdout: &str) -> Option<String> {
    let name = stdout.lines().next()?.split_whitespace().next()?;
    is_valid_toolchain_name(name).then(|| name.to_string())
}

fn is_valid_toolchain_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_'))
}

/// Channel pinned by a `rust-toolchain` / `rust-toolchain.toml` file body. Supports the TOML form
/// (`[toolchain] channel = "..."`) and the legacy single-line form.
fn parse_toolchain_file(body: &str) -> Option<String> {
    let mut in_toolchain = false;
    let mut saw_table = false;
    for raw in body.lines() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('[') {
            saw_table = true;
            in_toolchain = line == "[toolchain]";
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            if key.trim() == "channel" && (in_toolchain || !saw_table) {
                let value = value.trim().trim_matches(|c| c == '"' || c == '\'');
                return is_valid_toolchain_name(value).then(|| value.to_string());
            }
            continue;
        }
        if !saw_table && is_valid_toolchain_name(line) {
            return Some(line.to_string());
        }
    }
    None
}

fn pinned_channels(project_roots: &[PathBuf]) -> Vec<String> {
    let mut out = Vec::new();
    for root in project_roots {
        for file in ["rust-toolchain.toml", "rust-toolchain"] {
            if let Ok(body) = std::fs::read_to_string(root.join(file)) {
                if let Some(channel) = parse_toolchain_file(&body) {
                    out.push(channel);
                }
            }
        }
    }
    out
}

/// A toolchain matches a channel when equal or when the channel is a prefix followed by `-`
/// (`stable` → `stable-x86_64-pc-windows-msvc`). Over-matching only over-protects.
fn channel_matches(toolchain: &str, channel: &str) -> bool {
    toolchain == channel
        || toolchain
            .strip_prefix(channel)
            .is_some_and(|rest| rest.starts_with('-'))
}

fn classify_toolchains(
    listed: Vec<ListedToolchain>,
    overrides: &[String],
    active: Option<&str>,
    pinned: &[String],
    measure: &dyn Fn(&Path) -> u64,
) -> RustToolchainInventory {
    let toolchains: Vec<RustToolchainFact> = listed
        .into_iter()
        .map(|t| {
            let is_override = t.is_override || overrides.iter().any(|o| channel_matches(&t.name, o));
            let is_active = t.is_active || active.is_some_and(|a| a == t.name);
            let is_pinned = pinned.iter().any(|c| channel_matches(&t.name, c));
            let protected = t.is_default || is_active || is_override || is_pinned;
            let bytes = if protected {
                0
            } else {
                t.path.as_deref().map(measure).unwrap_or(0)
            };
            RustToolchainFact {
                name: t.name,
                is_default: t.is_default,
                is_active,
                is_override,
                is_pinned,
                protected,
                bytes,
                path: t.path,
            }
        })
        .collect();
    let removable_bytes = toolchains.iter().filter(|t| !t.protected).map(|t| t.bytes).sum();
    RustToolchainInventory {
        toolchains,
        removable_bytes,
    }
}

fn rustup_inventory(exe: &Path, project_roots: &[PathBuf]) -> DevToolResult<RustToolchainInventory> {
    let never = || false;
    let list = run_tool(exe, &["toolchain", "list", "-v"], RUSTUP_LIST_TIMEOUT, &never)?;
    if !list.success {
        return Err(failed(&list));
    }
    let listed = parse_toolchain_list(&list.stdout).ok_or(DevToolError::new(DevToolReason::InvalidOutput))?;

    let overrides = run_tool(exe, &["override", "list"], RUSTUP_LIST_TIMEOUT, &never)?;
    if !overrides.success {
        return Err(failed(&overrides));
    }
    let overrides =
        parse_override_list(&overrides.stdout).ok_or(DevToolError::new(DevToolReason::InvalidOutput))?;

    // Non-zero exit here means "no active toolchain"; the list flags still cover rustup ≥1.28.
    let active = run_tool(exe, &["show", "active-toolchain"], RUSTUP_LIST_TIMEOUT, &never)?;
    let active = active
        .success
        .then(|| parse_active_toolchain(&active.stdout))
        .flatten();

    let pinned = pinned_channels(project_roots);
    let home_toolchains = std::env::var_os("RUSTUP_HOME")
        .map(PathBuf::from)
        .or_else(|| home_dir().map(|h| h.join(".rustup")))
        .map(|h| h.join("toolchains"));
    let listed = listed
        .into_iter()
        .map(|mut t| {
            if t.path.is_none() {
                t.path = home_toolchains.as_ref().map(|d| d.join(&t.name));
            }
            t
        })
        .collect();
    Ok(classify_toolchains(listed, &overrides, active.as_deref(), &pinned, &dir_size))
}

/// Read-only: installed toolchains with protection flags and measured size of removable ones.
pub fn rust_toolchains_list(project_roots: &[PathBuf]) -> DevToolResult<RustToolchainInventory> {
    rustup_inventory(&rustup_exe()?, project_roots)
}

/// Destructive: uninstall the named toolchains. Protection is recomputed from a fresh listing;
/// protected or unknown names are refused per item. Cancel is honoured between toolchains only,
/// so a rustup uninstall is never killed half-way.
pub fn rust_toolchains_uninstall(
    names: &[String],
    project_roots: &[PathBuf],
    should_cancel: &dyn Fn() -> bool,
) -> DevToolResult<Vec<RustToolchainUninstall>> {
    let exe = rustup_exe()?;
    let inventory = rustup_inventory(&exe, project_roots)?;
    let mut results = Vec::new();
    let mut attempted = Vec::new();
    for name in names {
        let item = |status, released_bytes| RustToolchainUninstall {
            name: name.clone(),
            status,
            released_bytes,
        };
        if should_cancel() {
            results.push(item(RustToolchainUninstallStatus::Cancelled, 0));
            continue;
        }
        let Some(fact) = inventory.toolchains.iter().find(|t| &t.name == name) else {
            results.push(item(RustToolchainUninstallStatus::NotFound, 0));
            continue;
        };
        if fact.protected {
            results.push(item(RustToolchainUninstallStatus::Protected, 0));
            continue;
        }
        let output = run_tool(
            &exe,
            &["toolchain", "uninstall", name.as_str()],
            RUSTUP_UNINSTALL_TIMEOUT,
            &|| false,
        );
        match output {
            Ok(out) if out.success => {
                attempted.push(results.len());
                results.push(item(RustToolchainUninstallStatus::Removed, fact.bytes));
            }
            _ => results.push(item(RustToolchainUninstallStatus::Failed, 0)),
        }
    }
    if !attempted.is_empty() {
        let after = rustup_inventory(&exe, project_roots)?;
        for index in attempted {
            let result = &mut results[index];
            if after.toolchains.iter().any(|t| t.name == result.name) {
                result.status = RustToolchainUninstallStatus::StillPresent;
                result.released_bytes = 0;
            }
        }
    }
    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_sizes_parse_decimal_and_binary_units() {
        assert_eq!(parse_human_size("1.2GB"), Some(1_200_000_000));
        assert_eq!(parse_human_size("512kB"), Some(512_000));
        assert_eq!(parse_human_size("0B"), Some(0));
        assert_eq!(parse_human_size("3.5MB"), Some(3_500_000));
        assert_eq!(parse_human_size("1KiB"), Some(1024));
        assert_eq!(parse_human_size("2GiB"), Some(2 * 1024 * 1024 * 1024));
        assert_eq!(parse_human_size(" 42 "), Some(42));
        assert_eq!(parse_human_size("12 parsecs"), None);
        assert_eq!(parse_human_size("GB"), None);
        assert_eq!(parse_human_size(""), None);
    }

    #[test]
    fn docker_df_sums_only_unused_unshared_records() {
        let fixture = r#"{"Images":[],"Containers":[],"Volumes":[],"BuildCache":[
            {"ID":"a","CacheType":"regular","InUse":false,"Shared":false,"Size":"1.2GB"},
            {"ID":"b","CacheType":"regular","InUse":true,"Shared":false,"Size":"5GB"},
            {"ID":"c","CacheType":"source.local","InUse":false,"Shared":true,"Size":"7MB"},
            {"ID":"d","CacheType":"exec.cachemount","InUse":"false","Shared":"false","Size":"512kB"}]}"#
            .replace('\n', "");
        let preview = parse_docker_df(&fixture).expect("parse");
        assert_eq!(preview.reclaimable_bytes, 1_200_000_000 + 512_000);
        assert_eq!(preview.record_count, 2);
    }

    #[test]
    fn docker_df_accepts_null_and_empty_cache() {
        let preview = parse_docker_df(r#"{"Images":[],"BuildCache":null}"#).expect("parse");
        assert_eq!(preview.reclaimable_bytes, 0);
        let preview = parse_docker_df(r#"{"BuildCache":[]}"#).expect("parse");
        assert_eq!(preview.record_count, 0);
    }

    #[test]
    fn docker_df_accepts_legacy_json_lines() {
        let fixture = concat!(
            r#"{"Active":"2","Reclaimable":"1.5GB (60%)","Size":"2.5GB","TotalCount":"3","Type":"Images"}"#,
            "\n",
            r#"{"Active":"0","Reclaimable":"300MB (100%)","Size":"300MB","TotalCount":"12","Type":"Build Cache"}"#,
            "\n"
        );
        let preview = parse_docker_df(fixture).expect("parse");
        assert_eq!(preview.reclaimable_bytes, 300_000_000);
        assert_eq!(preview.record_count, 12);
    }

    #[test]
    fn docker_df_fails_closed_on_bad_output() {
        assert!(parse_docker_df("").is_none());
        assert!(parse_docker_df("not json").is_none());
        assert!(parse_docker_df(r#"{"Images":[]}"#).is_none());
        assert!(parse_docker_df(r#"{"BuildCache":[{"InUse":false,"Size":"huge"}]}"#).is_none());
        assert!(parse_docker_df(r#"{"BuildCache":[{"Size":"1GB"}]}"#).is_none());
    }

    #[test]
    fn docker_daemon_errors_are_classified() {
        let output = ToolOutput {
            exit_code: Some(1),
            success: false,
            stdout: String::new(),
            stderr: "failed to connect to the docker API at npipe:////./pipe/docker_engine".into(),
        };
        assert_eq!(docker_failure(&output).reason, DevToolReason::DaemonUnavailable);
        let output = ToolOutput {
            stderr: "unknown flag".into(),
            ..output
        };
        assert_eq!(docker_failure(&output).reason, DevToolReason::Failed);
    }

    #[test]
    fn conda_clean_json_is_parsed() {
        let fixture = r#"WARNING: something
{
  "index_cache": {"files": ["/opt/conda/pkgs/cache"]},
  "logfiles": {"pkgs_dirs": {}, "total_size": 2048, "warnings": []},
  "success": true,
  "tarballs": {"pkgs_dirs": {"/opt/conda/pkgs": ["a.conda"]}, "total_size": 1000000, "warnings": []}
}"#;
        let (preview, dirs) = parse_conda_clean(fixture).expect("parse");
        assert_eq!(preview.tarball_bytes, 1_000_000);
        assert_eq!(preview.logfile_bytes, 2048);
        assert_eq!(preview.total_bytes, 1_002_048);
        assert_eq!(dirs, vec![PathBuf::from("/opt/conda/pkgs/cache")]);
    }

    #[test]
    fn conda_clean_fails_closed() {
        assert!(parse_conda_clean("").is_none());
        assert!(parse_conda_clean(r#"{"success": false}"#).is_none());
        assert!(parse_conda_clean(r#"{"tarballs": {"total_size": "big"}}"#).is_none());
        let (preview, dirs) = parse_conda_clean(r#"{"success": true}"#).expect("empty ok");
        assert_eq!(preview, CondaCleanPreview::default());
        assert!(dirs.is_empty());
    }

    #[test]
    fn toolchain_list_parses_modern_and_legacy_output() {
        let modern = "stable-x86_64-pc-windows-gnu (active, default) C:\\Users\\u\\.rustup\\toolchains\\stable-x86_64-pc-windows-gnu\n\
                      stable-x86_64-pc-windows-msvc C:\\Users\\u\\.rustup\\toolchains\\stable-x86_64-pc-windows-msvc\n";
        let parsed = parse_toolchain_list(modern).expect("parse");
        assert_eq!(parsed.len(), 2);
        assert!(parsed[0].is_active && parsed[0].is_default);
        assert!(!parsed[1].is_active && !parsed[1].is_default);
        assert_eq!(
            parsed[1].path.as_deref(),
            Some(Path::new("C:\\Users\\u\\.rustup\\toolchains\\stable-x86_64-pc-windows-msvc"))
        );

        let legacy = "nightly-2024-01-01-x86_64-pc-windows-msvc\tC:\\Program Data\\r\\n1\n\
                      stable-x86_64-pc-windows-msvc (default)\tC:\\r\\s\n\
                      1.75.0-x86_64-pc-windows-msvc (override)\n";
        let parsed = parse_toolchain_list(legacy).expect("parse");
        assert_eq!(parsed[0].path.as_deref(), Some(Path::new("C:\\Program Data\\r\\n1")));
        assert!(parsed[1].is_default);
        assert!(parsed[2].is_override);
        assert!(parsed[2].path.is_none());

        assert_eq!(parse_toolchain_list("no installed toolchains\n"), Some(Vec::new()));
        assert!(parse_toolchain_list("error: something bad happened\n").is_none());
        assert!(parse_toolchain_list("stable (default C:\\x\n").is_none());
    }

    #[test]
    fn override_and_active_parse() {
        assert_eq!(parse_override_list("no overrides\n"), Some(Vec::new()));
        assert_eq!(
            parse_override_list("C:\\My Projects\\a\tnightly-x86_64-pc-windows-msvc\n"),
            Some(vec!["nightly-x86_64-pc-windows-msvc".to_string()])
        );
        assert!(parse_override_list("C:\\x\t<weird>\n").is_none());
        assert_eq!(
            parse_active_toolchain("stable-x86_64-pc-windows-msvc (default)\n").as_deref(),
            Some("stable-x86_64-pc-windows-msvc")
        );
        assert_eq!(parse_active_toolchain(""), None);
    }

    #[test]
    fn toolchain_files_parse() {
        assert_eq!(
            parse_toolchain_file("[toolchain]\nchannel = \"1.75.0\"\ncomponents = [\"clippy\"]\n").as_deref(),
            Some("1.75.0")
        );
        assert_eq!(parse_toolchain_file("nightly-2024-05-01\n").as_deref(), Some("nightly-2024-05-01"));
        assert_eq!(parse_toolchain_file("# comment\n[other]\nchannel = \"x\"\n"), None);
        assert_eq!(parse_toolchain_file(""), None);
    }

    #[test]
    fn protection_covers_default_active_override_and_pins() {
        let t = |name: &str, default: bool| ListedToolchain {
            name: name.into(),
            is_default: default,
            is_active: false,
            is_override: false,
            path: Some(PathBuf::from(name)),
        };
        let listed = vec![
            t("stable-x86_64-pc-windows-msvc", true),
            t("nightly-x86_64-pc-windows-msvc", false),
            t("1.75.0-x86_64-pc-windows-msvc", false),
            t("beta-x86_64-pc-windows-msvc", false),
            t("1.70.0-x86_64-pc-windows-msvc", false),
        ];
        let inventory = classify_toolchains(
            listed,
            &["nightly-x86_64-pc-windows-msvc".into()],
            Some("beta-x86_64-pc-windows-msvc"),
            &["1.75.0".into()],
            &|_| 100,
        );
        let protected: Vec<_> = inventory
            .toolchains
            .iter()
            .filter(|t| t.protected)
            .map(|t| t.name.as_str())
            .collect();
        assert_eq!(
            protected,
            vec![
                "stable-x86_64-pc-windows-msvc",
                "nightly-x86_64-pc-windows-msvc",
                "1.75.0-x86_64-pc-windows-msvc",
                "beta-x86_64-pc-windows-msvc",
            ]
        );
        assert_eq!(inventory.removable_bytes, 100);
        // Prefix matching requires a '-' boundary so "1.7" never pins "1.70.0-…".
        assert!(!channel_matches("1.70.0-x86_64-pc-windows-msvc", "1.7"));
        assert!(channel_matches("stable-x86_64-pc-windows-msvc", "stable"));
    }

    #[test]
    fn runner_reports_missing_tool() {
        let result = run_tool(
            Path::new("definitely-not-a-real-tool-pctoolkit.exe"),
            &[],
            Duration::from_secs(5),
            &|| false,
        );
        assert_eq!(result.err().map(|e| e.reason), Some(DevToolReason::ToolUnavailable));
    }

    /// Read-only smoke run against the real machine. Never prunes / cleans / uninstalls.
    #[test]
    #[ignore]
    fn smoke_previews() {
        let started = Instant::now();
        println!("docker: {:?} ms={}", docker_build_cache_preview(&|| false), started.elapsed().as_millis());
        let started = Instant::now();
        println!("conda: {:?} ms={}", conda_clean_preview(&|| false), started.elapsed().as_millis());
        let started = Instant::now();
        match rust_toolchains_list(&[]) {
            Ok(inv) => {
                for t in &inv.toolchains {
                    println!(
                        "rust: {} default={} active={} override={} pinned={} protected={} bytes={}",
                        t.name, t.is_default, t.is_active, t.is_override, t.is_pinned, t.protected, t.bytes
                    );
                }
                println!("rust removable_bytes={} ms={}", inv.removable_bytes, started.elapsed().as_millis());
            }
            Err(e) => println!("rust: {e:?}"),
        }
        println!("ollama running={}", count_running_images(&["ollama.exe", "ollama app.exe"]));
    }
}

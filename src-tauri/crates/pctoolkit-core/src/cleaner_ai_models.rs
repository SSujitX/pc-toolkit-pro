//! Local AI model storage discovery and removal (Hugging Face hub, Whisper, PyTorch hub, Keras,
//! LM Studio, Ollama, GPT4All, Jan).
//!
//! Models are large, slow to re-download, and never junk by default, so callers must never
//! preselect them. Safety rules:
//! - Link-like entries (symlinks, junctions / reparse points, cloud placeholders) are never
//!   followed or counted, and cloud placeholders are never deleted (that would delete the cloud copy).
//! - Deletion re-discovers the candidate from the provider's own root and requires an identical
//!   fingerprint, byte count and file count; a forged or stale candidate is refused.
//! - Stores owned by a running app (Ollama, LM Studio, GPT4All, Jan) are refused while it runs.
//! - Removal is permanent (models are too large for the Recycle Bin); files are removed one by one
//!   and directories pruned bottom-up, so a failure leaves a consistent partial state.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AiModelProvider {
    HuggingFace,
    Whisper,
    PyTorchHub,
    Keras,
    LmStudio,
    Ollama,
    Gpt4All,
    Jan,
}

impl AiModelProvider {
    pub const ALL: [Self; 8] = [
        Self::HuggingFace,
        Self::Whisper,
        Self::PyTorchHub,
        Self::Keras,
        Self::LmStudio,
        Self::Ollama,
        Self::Gpt4All,
        Self::Jan,
    ];

    /// Image names that hold the store open and must be closed before deletion.
    pub fn processes_to_close(self) -> &'static [&'static str] {
        match self {
            Self::Ollama => &["ollama.exe", "ollama app.exe"],
            Self::LmStudio => &["lm studio.exe", "lms.exe"],
            // GPT4All's window process is a generic "chat.exe"; only the unambiguous name is used.
            Self::Gpt4All => &["gpt4all.exe"],
            Self::Jan => &["jan.exe"],
            Self::HuggingFace | Self::Whisper | Self::PyTorchHub | Self::Keras => &[],
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::HuggingFace => "huggingFace",
            Self::Whisper => "whisper",
            Self::PyTorchHub => "pyTorchHub",
            Self::Keras => "keras",
            Self::LmStudio => "lmStudio",
            Self::Ollama => "ollama",
            Self::Gpt4All => "gpt4All",
            Self::Jan => "jan",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiModelCandidate {
    /// Stable within a process: provider + hash of the path. Frontends address candidates by id.
    pub id: String,
    pub provider: AiModelProvider,
    /// Model id derived from the folder / file name (for example `meta-llama/Llama-3-8B`).
    pub display_name: String,
    pub bytes: u64,
    pub file_count: u64,
    pub newest_mtime_ms: Option<i64>,
    /// Hash of (relative name, size, mtime) for every counted file. Serialized as a string so
    /// JavaScript does not lose precision.
    #[serde(serialize_with = "fingerprint_as_string")]
    pub fingerprint: u64,
    pub processes_to_close: Vec<String>,
    /// Core-held location; never sent to or accepted from the frontend.
    #[serde(skip)]
    pub path: PathBuf,
}

fn fingerprint_as_string<S: serde::Serializer>(value: &u64, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(&format!("{value:016x}"))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiModelDeleteResult {
    pub id: String,
    pub removed_bytes: u64,
    pub removed_files: u64,
    pub failed_files: u64,
    /// Link-like entries left in place (never followed or deleted through).
    pub skipped_links: u64,
    /// True when the candidate no longer exists on disk.
    pub complete: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, thiserror::Error)]
#[serde(rename_all = "camelCase", tag = "code")]
pub enum AiModelError {
    /// The candidate is no longer discoverable under its provider root.
    #[error("model not found")]
    NotFound,
    /// Size, file count or fingerprint differs from the reviewed candidate.
    #[error("model changed since scan")]
    Changed,
    #[error("model store is in use")]
    ProcessRunning { images: Vec<String> },
}

/// Provider roots. `from_env` resolves the real profile; tests pass temp directories.
#[derive(Debug, Clone, Default)]
pub struct AiModelRoots {
    pub hf_hub: Option<PathBuf>,
    pub whisper: Option<PathBuf>,
    pub torch_checkpoints: Option<PathBuf>,
    pub keras_models: Option<PathBuf>,
    pub lm_studio: Vec<PathBuf>,
    pub ollama_models: Option<PathBuf>,
    pub gpt4all: Option<PathBuf>,
    pub jan_models: Option<PathBuf>,
}

fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

impl AiModelRoots {
    pub fn from_env() -> Self {
        let home = env_path("USERPROFILE").or_else(|| env_path("HOME"));
        let cache = home.as_ref().map(|h| h.join(".cache"));
        Self {
            hf_hub: env_path("HF_HUB_CACHE")
                .or_else(|| env_path("HUGGINGFACE_HUB_CACHE"))
                .or_else(|| env_path("HF_HOME").map(|p| p.join("hub")))
                .or_else(|| cache.as_ref().map(|c| c.join("huggingface").join("hub"))),
            whisper: cache.as_ref().map(|c| c.join("whisper")),
            torch_checkpoints: env_path("TORCH_HOME")
                .or_else(|| cache.as_ref().map(|c| c.join("torch")))
                .map(|t| t.join("hub").join("checkpoints")),
            keras_models: env_path("KERAS_HOME")
                .or_else(|| home.as_ref().map(|h| h.join(".keras")))
                .map(|k| k.join("models")),
            lm_studio: [
                home.as_ref().map(|h| h.join(".lmstudio").join("models")),
                cache.as_ref().map(|c| c.join("lm-studio").join("models")),
            ]
            .into_iter()
            .flatten()
            .collect(),
            ollama_models: env_path("OLLAMA_MODELS")
                .or_else(|| home.as_ref().map(|h| h.join(".ollama").join("models"))),
            gpt4all: env_path("LOCALAPPDATA").map(|l| l.join("nomic.ai").join("GPT4All")),
            jan_models: env_path("APPDATA").map(|a| a.join("Jan").join("data").join("models")),
        }
    }
}

// ------------------------------------------------------------------------------------------------
// Link-like detection

const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
const FILE_ATTRIBUTE_OFFLINE: u32 = 0x1000;
const FILE_ATTRIBUTE_RECALL_ON_OPEN: u32 = 0x4_0000;
const FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS: u32 = 0x40_0000;

fn link_like_attributes(attributes: u32) -> bool {
    attributes
        & (FILE_ATTRIBUTE_REPARSE_POINT
            | FILE_ATTRIBUTE_OFFLINE
            | FILE_ATTRIBUTE_RECALL_ON_OPEN
            | FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS)
        != 0
}

fn is_link_like(meta: &std::fs::Metadata) -> bool {
    if meta.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        link_like_attributes(meta.file_attributes())
    }
    #[cfg(not(windows))]
    {
        false
    }
}

/// Real (non-link) directory / file metadata, or `None`.
fn real_meta(path: &Path) -> Option<std::fs::Metadata> {
    std::fs::symlink_metadata(path).ok().filter(|m| !is_link_like(m))
}

// ------------------------------------------------------------------------------------------------
// Tree measurement

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TreeStats {
    bytes: u64,
    files: u64,
    newest_mtime_ms: Option<i64>,
    fingerprint: u64,
}

fn mtime_ms(meta: &std::fs::Metadata) -> Option<i64> {
    let since = meta.modified().ok()?.duration_since(UNIX_EPOCH).ok()?;
    i64::try_from(since.as_millis()).ok()
}

/// Measure a file or directory tree without following links. `None` when cancelled or unreadable.
fn measure(root: &Path, should_cancel: &dyn Fn() -> bool) -> Option<TreeStats> {
    let meta = real_meta(root)?;
    let mut entries: Vec<(String, u64, i64)> = Vec::new();
    if meta.is_file() {
        let name = root.file_name()?.to_string_lossy().into_owned();
        entries.push((name, meta.len(), mtime_ms(&meta).unwrap_or(0)));
    } else {
        let mut stack = vec![root.to_path_buf()];
        while let Some(dir) = stack.pop() {
            if should_cancel() {
                return None;
            }
            let Ok(read) = std::fs::read_dir(&dir) else {
                continue;
            };
            for entry in read.flatten() {
                let path = entry.path();
                let Some(meta) = real_meta(&path) else {
                    continue;
                };
                if meta.is_dir() {
                    stack.push(path);
                } else if meta.is_file() {
                    let rel = path
                        .strip_prefix(root)
                        .unwrap_or(&path)
                        .to_string_lossy()
                        .into_owned();
                    entries.push((rel, meta.len(), mtime_ms(&meta).unwrap_or(0)));
                }
            }
        }
    }
    entries.sort();
    let mut hasher = DefaultHasher::new();
    let mut bytes = 0u64;
    let mut newest: Option<i64> = None;
    for (name, size, mtime) in &entries {
        (name, size, mtime).hash(&mut hasher);
        bytes = bytes.saturating_add(*size);
        newest = Some(newest.map_or(*mtime, |n| n.max(*mtime)));
    }
    Some(TreeStats {
        bytes,
        files: entries.len() as u64,
        newest_mtime_ms: newest,
        fingerprint: hasher.finish(),
    })
}

// ------------------------------------------------------------------------------------------------
// Discovery

fn candidate_id(provider: AiModelProvider, path: &Path) -> String {
    let mut hasher = DefaultHasher::new();
    path.to_string_lossy().to_lowercase().hash(&mut hasher);
    format!("{}:{:016x}", provider.label(), hasher.finish())
}

fn make_candidate(
    provider: AiModelProvider,
    path: PathBuf,
    display_name: String,
    should_cancel: &dyn Fn() -> bool,
) -> Option<AiModelCandidate> {
    let stats = measure(&path, should_cancel)?;
    if stats.bytes == 0 || stats.files == 0 {
        return None;
    }
    Some(AiModelCandidate {
        id: candidate_id(provider, &path),
        provider,
        display_name,
        bytes: stats.bytes,
        file_count: stats.files,
        newest_mtime_ms: stats.newest_mtime_ms,
        fingerprint: stats.fingerprint,
        processes_to_close: provider.processes_to_close().iter().map(|s| s.to_string()).collect(),
        path,
    })
}

/// Real child entries of a real directory, sorted by name.
fn children(dir: &Path) -> Vec<(PathBuf, String, std::fs::Metadata)> {
    if real_meta(dir).is_none_or(|m| !m.is_dir()) {
        return Vec::new();
    }
    let Ok(read) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<_> = read
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            let meta = real_meta(&path)?;
            let name = e.file_name().to_string_lossy().into_owned();
            Some((path, name, meta))
        })
        .collect();
    out.sort_by(|a, b| a.1.cmp(&b.1));
    out
}

fn has_extension(name: &str, extensions: &[&str]) -> bool {
    Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| extensions.iter().any(|x| x.eq_ignore_ascii_case(e)))
}

fn file_stem(name: &str) -> String {
    Path::new(name)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| name.to_string())
}

/// `models--meta-llama--Llama-3-8B` → `meta-llama/Llama-3-8B`.
fn hf_display_name(folder: &str) -> Option<String> {
    let rest = folder.strip_prefix("models--")?;
    (!rest.is_empty()).then(|| rest.replace("--", "/"))
}

fn files_with_extensions(
    provider: AiModelProvider,
    root: Option<&PathBuf>,
    extensions: &[&str],
    should_cancel: &dyn Fn() -> bool,
) -> Vec<AiModelCandidate> {
    let Some(root) = root else {
        return Vec::new();
    };
    children(root)
        .into_iter()
        .filter(|(_, name, meta)| meta.is_file() && has_extension(name, extensions))
        .filter_map(|(path, name, _)| make_candidate(provider, path, file_stem(&name), should_cancel))
        .collect()
}

fn discover_provider(
    roots: &AiModelRoots,
    provider: AiModelProvider,
    should_cancel: &dyn Fn() -> bool,
) -> Vec<AiModelCandidate> {
    match provider {
        AiModelProvider::HuggingFace => roots
            .hf_hub
            .iter()
            .flat_map(|root| children(root))
            .filter(|(_, _, meta)| meta.is_dir())
            .filter_map(|(path, name, _)| {
                let display = hf_display_name(&name)?;
                make_candidate(provider, path, display, should_cancel)
            })
            .collect(),
        AiModelProvider::Whisper => files_with_extensions(provider, roots.whisper.as_ref(), &["pt"], should_cancel),
        AiModelProvider::PyTorchHub => files_with_extensions(
            provider,
            roots.torch_checkpoints.as_ref(),
            &["pth", "pt", "ckpt", "bin", "safetensors"],
            should_cancel,
        ),
        AiModelProvider::Keras => files_with_extensions(
            provider,
            roots.keras_models.as_ref(),
            &["h5", "hdf5", "keras"],
            should_cancel,
        ),
        AiModelProvider::Gpt4All => files_with_extensions(provider, roots.gpt4all.as_ref(), &["gguf"], should_cancel),
        AiModelProvider::LmStudio => {
            let mut out = Vec::new();
            for root in &roots.lm_studio {
                for (publisher_path, publisher, meta) in children(root) {
                    if !meta.is_dir() || publisher.starts_with('.') {
                        continue;
                    }
                    for (model_path, model, meta) in children(&publisher_path) {
                        if !meta.is_dir() || model.starts_with('.') {
                            continue;
                        }
                        let display = format!("{publisher}/{model}");
                        out.extend(make_candidate(provider, model_path, display, should_cancel));
                    }
                }
            }
            out
        }
        AiModelProvider::Ollama => {
            let Some(root) = roots.ollama_models.as_ref() else {
                return Vec::new();
            };
            // Blobs are content-addressed and shared between tags; only the whole store is safe.
            let is_store = ["blobs", "manifests"]
                .iter()
                .all(|d| real_meta(&root.join(d)).is_some_and(|m| m.is_dir()));
            if !is_store {
                return Vec::new();
            }
            make_candidate(provider, root.clone(), "Ollama models".to_string(), should_cancel)
                .into_iter()
                .collect()
        }
        AiModelProvider::Jan => roots
            .jan_models
            .iter()
            .flat_map(|root| children(root))
            .filter(|(_, name, meta)| meta.is_dir() && !name.starts_with('.'))
            .filter_map(|(path, name, _)| make_candidate(provider, path, name, should_cancel))
            .collect(),
    }
}

fn discover_in(roots: &AiModelRoots, should_cancel: &dyn Fn() -> bool) -> Vec<AiModelCandidate> {
    let mut out = Vec::new();
    for provider in AiModelProvider::ALL {
        if should_cancel() {
            break;
        }
        out.extend(discover_provider(roots, provider, should_cancel));
    }
    out
}

/// Discover model storage in the current user's profile. Read-only; returns what was found before
/// a cancel.
pub fn discover_ai_models(should_cancel: &dyn Fn() -> bool) -> Vec<AiModelCandidate> {
    discover_in(&AiModelRoots::from_env(), should_cancel)
}

// ------------------------------------------------------------------------------------------------
// Deletion

/// Ollama's candidate is the store root itself; keep the (then empty) folder so the app does not
/// need to recreate its configured location.
fn keeps_root(provider: AiModelProvider) -> bool {
    provider == AiModelProvider::Ollama
}

fn delete_tree(root: &Path, keep_root: bool, result: &mut AiModelDeleteResult) {
    let Some(meta) = real_meta(root) else {
        result.skipped_links += 1;
        return;
    };
    if meta.is_file() {
        match std::fs::remove_file(root) {
            Ok(()) => {
                result.removed_bytes += meta.len();
                result.removed_files += 1;
            }
            Err(_) => result.failed_files += 1,
        }
        return;
    }
    let mut dirs = vec![root.to_path_buf()];
    let mut index = 0;
    while index < dirs.len() {
        let dir = dirs[index].clone();
        index += 1;
        let Ok(read) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in read.flatten() {
            let path = entry.path();
            let Ok(meta) = std::fs::symlink_metadata(&path) else {
                continue;
            };
            if meta.file_type().is_symlink() && !meta.is_dir() && !cloud_placeholder(&meta) {
                // A file symlink (Hugging Face snapshots → blobs): remove the link itself only.
                if std::fs::remove_file(&path).is_err() {
                    result.skipped_links += 1;
                }
                continue;
            }
            if is_link_like(&meta) {
                result.skipped_links += 1;
                continue;
            }
            if meta.is_dir() {
                dirs.push(path);
            } else {
                match std::fs::remove_file(&path) {
                    Ok(()) => {
                        result.removed_bytes += meta.len();
                        result.removed_files += 1;
                    }
                    Err(_) => result.failed_files += 1,
                }
            }
        }
    }
    // Deepest first; non-empty directories (failed / skipped entries) simply stay.
    for dir in dirs.iter().rev() {
        if keep_root && dir == root {
            continue;
        }
        let _ = std::fs::remove_dir(dir);
    }
}

fn cloud_placeholder(meta: &std::fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        meta.file_attributes()
            & (FILE_ATTRIBUTE_OFFLINE | FILE_ATTRIBUTE_RECALL_ON_OPEN | FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS)
            != 0
    }
    #[cfg(not(windows))]
    {
        let _ = meta;
        false
    }
}

fn delete_in(
    roots: &AiModelRoots,
    candidate: &AiModelCandidate,
    running_count: &dyn Fn(&[&str]) -> u32,
) -> Result<AiModelDeleteResult, AiModelError> {
    let images = candidate.provider.processes_to_close();
    if !images.is_empty() && running_count(images) > 0 {
        return Err(AiModelError::ProcessRunning {
            images: images.iter().map(|s| s.to_string()).collect(),
        });
    }
    // Re-discover from the provider root: this both refreshes the fingerprint and rejects a
    // candidate whose path does not belong to the provider.
    let current = discover_provider(roots, candidate.provider, &|| false)
        .into_iter()
        .find(|c| c.id == candidate.id)
        .ok_or(AiModelError::NotFound)?;
    if current.fingerprint != candidate.fingerprint
        || current.bytes != candidate.bytes
        || current.file_count != candidate.file_count
    {
        return Err(AiModelError::Changed);
    }
    let mut result = AiModelDeleteResult {
        id: current.id.clone(),
        removed_bytes: 0,
        removed_files: 0,
        failed_files: 0,
        skipped_links: 0,
        complete: false,
    };
    delete_tree(&current.path, keeps_root(current.provider), &mut result);
    result.complete = if keeps_root(current.provider) {
        std::fs::read_dir(&current.path).map_or(true, |mut r| r.next().is_none())
    } else {
        std::fs::symlink_metadata(&current.path).is_err()
    };
    Ok(result)
}

/// Permanently delete one previously discovered model. Destructive: requires explicit user
/// confirmation upstream. Refuses stale/forged candidates and stores held open by a running app.
pub fn delete_ai_model(candidate: &AiModelCandidate) -> Result<AiModelDeleteResult, AiModelError> {
    delete_in(&AiModelRoots::from_env(), candidate, &|images| {
        pctoolkit_platform::count_running_images(images)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicU32, Ordering};

    struct TempRoot(PathBuf);

    impl TempRoot {
        fn new(tag: &str) -> Self {
            static NEXT: AtomicU32 = AtomicU32::new(0);
            let path = std::env::temp_dir().join(format!(
                "pctoolkit-ai-models-{tag}-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn write(&self, rel: &str, bytes: usize) -> PathBuf {
            let path = self.0.join(rel);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, vec![7u8; bytes]).unwrap();
            path
        }
    }

    impl Drop for TempRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn roots_for(t: &TempRoot) -> AiModelRoots {
        AiModelRoots {
            hf_hub: Some(t.0.join("hf")),
            whisper: Some(t.0.join("whisper")),
            torch_checkpoints: Some(t.0.join("torch/hub/checkpoints")),
            keras_models: Some(t.0.join("keras/models")),
            lm_studio: vec![t.0.join("lmstudio/models")],
            ollama_models: Some(t.0.join("ollama/models")),
            gpt4all: Some(t.0.join("gpt4all")),
            jan_models: Some(t.0.join("jan/models")),
        }
    }

    fn never() -> bool {
        false
    }

    #[test]
    fn link_like_attribute_mask() {
        assert!(link_like_attributes(0x400));
        assert!(link_like_attributes(0x1000));
        assert!(link_like_attributes(0x4_0000));
        assert!(link_like_attributes(0x40_0000));
        assert!(!link_like_attributes(0x20 | 0x10 | 0x80));
    }

    #[test]
    fn hf_names_are_derived_from_folders() {
        assert_eq!(hf_display_name("models--meta-llama--Llama-3-8B").as_deref(), Some("meta-llama/Llama-3-8B"));
        assert_eq!(hf_display_name("models--gpt2").as_deref(), Some("gpt2"));
        assert_eq!(hf_display_name("datasets--squad"), None);
        assert_eq!(hf_display_name("models--"), None);
    }

    #[test]
    fn discovers_every_provider_layout() {
        let t = TempRoot::new("discover");
        t.write("hf/models--meta-llama--Llama-3-8B/blobs/abc", 100);
        t.write("hf/models--meta-llama--Llama-3-8B/snapshots/r1/config.json", 10);
        t.write("hf/datasets--squad/blobs/x", 50);
        t.write("hf/models--empty/refs/main", 0);
        t.write("whisper/base.pt", 30);
        t.write("whisper/notes.txt", 5);
        t.write("torch/hub/checkpoints/resnet50.pth", 40);
        t.write("keras/models/vgg16.h5", 20);
        t.write("keras/models/readme.md", 1);
        t.write("lmstudio/models/TheBloke/Mistral-7B-GGUF/model.gguf", 60);
        t.write("lmstudio/models/loose.gguf", 9);
        t.write("ollama/models/blobs/sha256-1", 70);
        t.write("ollama/models/manifests/registry/library/llama3/latest", 2);
        t.write("gpt4all/orca-mini.gguf", 25);
        t.write("gpt4all/localdocs.db", 3);
        t.write("jan/models/llama3-8b/model.gguf", 15);

        let found = discover_in(&roots_for(&t), &never);
        let mut names: Vec<_> = found
            .iter()
            .map(|c| (c.provider, c.display_name.as_str(), c.bytes, c.file_count))
            .collect();
        names.sort_by_key(|n| (format!("{:?}", n.0), n.1.to_string()));
        assert_eq!(
            names,
            vec![
                (AiModelProvider::Gpt4All, "orca-mini", 25, 1),
                (AiModelProvider::HuggingFace, "meta-llama/Llama-3-8B", 110, 2),
                (AiModelProvider::Jan, "llama3-8b", 15, 1),
                (AiModelProvider::Keras, "vgg16", 20, 1),
                (AiModelProvider::LmStudio, "TheBloke/Mistral-7B-GGUF", 60, 1),
                (AiModelProvider::Ollama, "Ollama models", 72, 2),
                (AiModelProvider::PyTorchHub, "resnet50", 40, 1),
                (AiModelProvider::Whisper, "base", 30, 1),
            ]
        );
        let ollama = found.iter().find(|c| c.provider == AiModelProvider::Ollama).unwrap();
        assert_eq!(ollama.processes_to_close, vec!["ollama.exe", "ollama app.exe"]);
        assert!(found.iter().all(|c| c.newest_mtime_ms.is_some()));
    }

    #[test]
    fn ollama_requires_blobs_and_manifests() {
        let t = TempRoot::new("ollama");
        t.write("ollama/models/blobs/sha256-1", 70);
        let found = discover_provider(&roots_for(&t), AiModelProvider::Ollama, &never);
        assert!(found.is_empty());
    }

    #[test]
    fn missing_roots_and_cancel_yield_nothing() {
        let t = TempRoot::new("missing");
        assert!(discover_in(&roots_for(&t), &never).is_empty());
        assert!(discover_in(&AiModelRoots::default(), &never).is_empty());
        t.write("whisper/base.pt", 30);
        assert!(discover_in(&roots_for(&t), &|| true).is_empty());
    }

    #[test]
    fn fingerprint_tracks_content_changes() {
        let t = TempRoot::new("fingerprint");
        let file = t.write("hf/models--a--b/blobs/x", 10);
        let first = discover_provider(&roots_for(&t), AiModelProvider::HuggingFace, &never);
        let again = discover_provider(&roots_for(&t), AiModelProvider::HuggingFace, &never);
        assert_eq!(first, again);
        fs::write(&file, vec![1u8; 11]).unwrap();
        let changed = discover_provider(&roots_for(&t), AiModelProvider::HuggingFace, &never);
        assert_ne!(first[0].fingerprint, changed[0].fingerprint);
        assert_eq!(first[0].id, changed[0].id);
    }

    #[test]
    fn delete_removes_directory_model() {
        let t = TempRoot::new("delete-dir");
        t.write("hf/models--a--b/blobs/x", 10);
        t.write("hf/models--a--b/snapshots/r/c.json", 5);
        t.write("hf/models--keep--me/blobs/y", 7);
        let roots = roots_for(&t);
        let found = discover_provider(&roots, AiModelProvider::HuggingFace, &never);
        let target = found.iter().find(|c| c.display_name == "a/b").unwrap();
        let result = delete_in(&roots, target, &|_| 0).expect("delete");
        assert_eq!(result.removed_bytes, 15);
        assert_eq!(result.removed_files, 2);
        assert_eq!(result.failed_files, 0);
        assert!(result.complete);
        assert!(!t.0.join("hf/models--a--b").exists());
        assert!(t.0.join("hf/models--keep--me/blobs/y").exists());
        assert!(t.0.join("hf").is_dir());
    }

    #[test]
    fn delete_removes_single_file_model() {
        let t = TempRoot::new("delete-file");
        t.write("whisper/base.pt", 30);
        t.write("whisper/notes.txt", 5);
        let roots = roots_for(&t);
        let target = discover_provider(&roots, AiModelProvider::Whisper, &never).remove(0);
        let result = delete_in(&roots, &target, &|_| 0).expect("delete");
        assert_eq!((result.removed_bytes, result.complete), (30, true));
        assert!(t.0.join("whisper/notes.txt").exists());
    }

    #[test]
    fn delete_keeps_ollama_store_root() {
        let t = TempRoot::new("delete-ollama");
        t.write("ollama/models/blobs/sha256-1", 70);
        t.write("ollama/models/manifests/registry/library/llama3/latest", 2);
        let roots = roots_for(&t);
        let target = discover_provider(&roots, AiModelProvider::Ollama, &never).remove(0);
        let result = delete_in(&roots, &target, &|_| 0).expect("delete");
        assert_eq!(result.removed_bytes, 72);
        assert!(result.complete);
        assert!(t.0.join("ollama/models").is_dir());
        assert!(!t.0.join("ollama/models/blobs").exists());
    }

    #[test]
    fn delete_refuses_running_process() {
        let t = TempRoot::new("delete-running");
        t.write("ollama/models/blobs/sha256-1", 70);
        t.write("ollama/models/manifests/m", 2);
        let roots = roots_for(&t);
        let target = discover_provider(&roots, AiModelProvider::Ollama, &never).remove(0);
        let err = delete_in(&roots, &target, &|images| {
            assert!(images.contains(&"ollama.exe"));
            1
        })
        .unwrap_err();
        assert!(matches!(err, AiModelError::ProcessRunning { .. }));
        assert!(t.0.join("ollama/models/blobs/sha256-1").exists());
    }

    #[test]
    fn delete_refuses_changed_candidate() {
        let t = TempRoot::new("delete-changed");
        let file = t.write("jan/models/m1/model.gguf", 15);
        let roots = roots_for(&t);
        let target = discover_provider(&roots, AiModelProvider::Jan, &never).remove(0);
        fs::write(&file, vec![0u8; 16]).unwrap();
        assert_eq!(delete_in(&roots, &target, &|_| 0), Err(AiModelError::Changed));
        assert!(file.exists());
    }

    #[test]
    fn delete_refuses_forged_or_missing_candidate() {
        let t = TempRoot::new("delete-forged");
        t.write("whisper/base.pt", 30);
        let outside = t.write("elsewhere/important.pt", 30);
        let roots = roots_for(&t);
        let mut forged = discover_provider(&roots, AiModelProvider::Whisper, &never).remove(0);
        forged.path = outside.clone();
        forged.id = candidate_id(AiModelProvider::Whisper, &outside);
        assert_eq!(delete_in(&roots, &forged, &|_| 0), Err(AiModelError::NotFound));
        assert!(outside.exists());

        let real = discover_provider(&roots, AiModelProvider::Whisper, &never).remove(0);
        fs::remove_file(t.0.join("whisper/base.pt")).unwrap();
        assert_eq!(delete_in(&roots, &real, &|_| 0), Err(AiModelError::NotFound));
    }

    /// Read-only smoke run against the real profile. Never deletes.
    #[test]
    #[ignore]
    fn smoke_discover_real_profile() {
        let started = std::time::Instant::now();
        let found = discover_ai_models(&never);
        for c in &found {
            println!("{:?} {} bytes={} files={}", c.provider, c.display_name, c.bytes, c.file_count);
        }
        println!("candidates={} ms={}", found.len(), started.elapsed().as_millis());
    }
}

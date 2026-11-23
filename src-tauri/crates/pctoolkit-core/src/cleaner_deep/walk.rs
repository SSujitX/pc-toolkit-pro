//! Safe directory walk: measure and clean under one validated root.
//!
//! Invariants:
//! - link-like entries (symlinks, junctions, reparse points, cloud placeholders)
//!   are never followed and never deleted;
//! - subtrees owned by a more specific rule are skipped;
//! - files are deleted one by one after a fresh metadata + matcher re-check, so
//!   partial failures count correctly and freed bytes are real;
//! - emptied directories are pruned bottom-up; in `contents` mode the root itself
//!   is never removed.
//!
//! Sizes are logical file lengths. Hardlinks are not deduplicated: reading the
//! file index needs an open handle per file (too costly for million-file caches),
//! so a hardlinked store (for example pnpm) can report more than deleting frees.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::time::SystemTime;

use super::matcher::{FileFacts, MatcherSpec};
use super::safety::{is_link_like, path_key, validate_root, RootPolicy, RootRefusal, SafetyContext};
use crate::cleaner::CANCELLED;

/// Per-rule safety limit on visited entries. Hitting it marks the rule `limited`.
pub const MAX_ENTRIES_PER_RULE: u64 = 5_000_000;
/// Directory nesting beyond this is not entered (keeps recursion bounded).
const MAX_WALK_DEPTH: u32 = 256;

const ERROR_SHARING_VIOLATION: i32 = 32;
const ERROR_LOCK_VIOLATION: i32 = 33;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CleanMode {
    /// Delete matching files under the root; prune emptied folders; keep the root.
    #[default]
    Contents,
    /// Like `contents`, then remove the root folder itself when it ends up empty.
    WholeRoot,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MeasureTally {
    pub bytes: u64,
    pub files: u64,
    /// Entries visited (files + folders), counted against the safety limit.
    pub entries: u64,
    pub limited: bool,
    pub cancelled: bool,
}

impl MeasureTally {
    pub fn absorb(&mut self, other: MeasureTally) {
        self.bytes += other.bytes;
        self.files += other.files;
        self.entries += other.entries;
        self.limited |= other.limited;
        self.cancelled |= other.cancelled;
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanTally {
    /// Sum of live sizes of files actually removed.
    pub freed_bytes: u64,
    pub removed: u64,
    /// Items that failed for reasons other than being in use (access denied, ...).
    pub failed: u64,
    /// Items held open by another process (sharing / lock violation).
    pub busy: u64,
    pub dirs_removed: u64,
    pub cancelled: bool,
}

impl CleanTally {
    pub fn absorb(&mut self, other: CleanTally) {
        self.freed_bytes += other.freed_bytes;
        self.removed += other.removed;
        self.failed += other.failed;
        self.busy += other.busy;
        self.dirs_removed += other.dirs_removed;
        self.cancelled |= other.cancelled;
    }
}

/// What one walk matches and which subtrees it must leave alone.
pub(crate) struct WalkSpec<'a> {
    pub matcher: &'a MatcherSpec,
    /// `path_key`s of folders owned by other rules.
    pub excluded: &'a HashSet<String>,
}

fn cancelled() -> bool {
    CANCELLED.load(Ordering::Relaxed)
}

fn lower_name(name: &std::ffi::OsStr) -> String {
    name.to_string_lossy().to_lowercase()
}

fn extension_of(name_lower: &str) -> &str {
    match name_lower.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => ext,
        _ => "",
    }
}

fn file_matches(
    matcher: &MatcherSpec,
    name_lower: &str,
    rel_dirs: &[String],
    meta: &fs::Metadata,
    now: SystemTime,
) -> bool {
    let age = if matcher.uses_age() {
        meta.modified().ok().and_then(|m| now.duration_since(m).ok())
    } else {
        None
    };
    matcher.matches(&FileFacts {
        name: name_lower,
        extension: extension_of(name_lower),
        rel_dirs,
        age,
    })
}

fn may_descend(matcher: &MatcherSpec, depth: u32) -> bool {
    depth < MAX_WALK_DEPTH && matcher.max_depth.is_none_or(|max| depth < max)
}

/// Measure matching files under `root` (already validated). `budget` caps visited entries.
pub(crate) fn measure_root(
    root: &Path,
    spec: &WalkSpec<'_>,
    budget: u64,
    now: SystemTime,
    on_file: &mut dyn FnMut(&Path, u64),
) -> MeasureTally {
    let mut tally = MeasureTally::default();
    let track_rel = spec.matcher.uses_path_segments();
    let mut stack: Vec<(PathBuf, u32, Vec<String>)> = vec![(root.to_path_buf(), 0, Vec::new())];
    while let Some((dir, depth, rel)) = stack.pop() {
        if cancelled() {
            tally.cancelled = true;
            break;
        }
        let Ok(read) = fs::read_dir(&dir) else {
            continue; // Denied or vanished: skip and continue.
        };
        for entry in read.flatten() {
            if tally.entries >= budget {
                tally.limited = true;
                return tally;
            }
            tally.entries += 1;
            let Ok(meta) = entry.metadata() else {
                continue;
            };
            if is_link_like(&meta) {
                continue;
            }
            let name = lower_name(&entry.file_name());
            if meta.is_dir() {
                let path = entry.path();
                if may_descend(spec.matcher, depth) && !spec.excluded.contains(&path_key(&path)) {
                    let mut child_rel = Vec::new();
                    if track_rel {
                        child_rel = rel.clone();
                        child_rel.push(name);
                    }
                    stack.push((path, depth + 1, child_rel));
                }
            } else if meta.is_file() && file_matches(spec.matcher, &name, &rel, &meta, now) {
                let len = meta.len();
                tally.bytes += len;
                tally.files += 1;
                on_file(&entry.path(), len);
            }
        }
    }
    tally
}

enum FileOutcome {
    Removed(u64),
    Busy,
    Failed,
    /// Gone, replaced by a link, or no longer matching: not ours to count.
    Skipped,
}

fn classify_error(error: &std::io::Error) -> FileOutcome {
    match error.raw_os_error() {
        Some(ERROR_SHARING_VIOLATION) | Some(ERROR_LOCK_VIOLATION) => FileOutcome::Busy,
        _ if error.kind() == ErrorKind::NotFound => FileOutcome::Skipped,
        _ => FileOutcome::Failed,
    }
}

/// Re-check a file right before deleting it, then delete it.
fn delete_file_checked(
    path: &Path,
    matcher: &MatcherSpec,
    name_lower: &str,
    rel_dirs: &[String],
    now: SystemTime,
) -> FileOutcome {
    let meta = match fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(e) => return classify_error(&e),
    };
    if is_link_like(&meta) || !meta.is_file() || !file_matches(matcher, name_lower, rel_dirs, &meta, now) {
        return FileOutcome::Skipped;
    }
    let len = meta.len();
    match fs::remove_file(path) {
        Ok(()) => FileOutcome::Removed(len),
        Err(e) if e.kind() == ErrorKind::PermissionDenied && meta.permissions().readonly() => {
            // Cache files are sometimes marked read-only; clear the bit once and retry.
            let mut perms = meta.permissions();
            #[allow(clippy::permissions_set_readonly_false)]
            perms.set_readonly(false);
            if fs::set_permissions(path, perms).is_err() {
                return FileOutcome::Failed;
            }
            match fs::remove_file(path) {
                Ok(()) => FileOutcome::Removed(len),
                Err(e) => classify_error(&e),
            }
        }
        Err(e) => classify_error(&e),
    }
}

struct CleanRun<'a, 'b> {
    spec: &'a WalkSpec<'b>,
    now: SystemTime,
    track_rel: bool,
    tally: CleanTally,
    on_removed: &'a mut dyn FnMut(&Path, u64),
}

impl CleanRun<'_, '_> {
    /// Returns true when anything below `dir` was removed.
    fn clean_dir(&mut self, dir: &Path, depth: u32, rel: &mut Vec<String>) -> bool {
        if cancelled() {
            self.tally.cancelled = true;
            return false;
        }
        let read = match fs::read_dir(dir) {
            Ok(read) => read,
            Err(e) if e.kind() == ErrorKind::NotFound => return false,
            Err(_) => {
                // A folder we cannot list is an item we could not clean.
                self.tally.failed += 1;
                return false;
            }
        };
        // Collect first so deletions do not disturb the directory enumeration.
        let entries: Vec<_> = read.flatten().collect();
        let mut removed_any = false;
        for entry in entries {
            if cancelled() {
                self.tally.cancelled = true;
                break;
            }
            let Ok(meta) = entry.metadata() else {
                continue;
            };
            if is_link_like(&meta) {
                continue;
            }
            let path = entry.path();
            let name = lower_name(&entry.file_name());
            if meta.is_dir() {
                if !may_descend(self.spec.matcher, depth) || self.spec.excluded.contains(&path_key(&path)) {
                    continue;
                }
                if self.track_rel {
                    rel.push(name);
                }
                let child_removed = self.clean_dir(&path, depth + 1, rel);
                if self.track_rel {
                    rel.pop();
                }
                if child_removed {
                    removed_any = true;
                    if prune_dir(&path) {
                        self.tally.dirs_removed += 1;
                    }
                }
            } else if meta.is_file() {
                if !file_matches(self.spec.matcher, &name, rel, &meta, self.now) {
                    continue;
                }
                match delete_file_checked(&path, self.spec.matcher, &name, rel, self.now) {
                    FileOutcome::Removed(len) => {
                        self.tally.freed_bytes += len;
                        self.tally.removed += 1;
                        removed_any = true;
                        (self.on_removed)(&path, len);
                    }
                    FileOutcome::Busy => self.tally.busy += 1,
                    FileOutcome::Failed => self.tally.failed += 1,
                    FileOutcome::Skipped => {}
                }
            }
        }
        removed_any
    }
}

/// Remove `dir` only if it is still a real, empty directory.
fn prune_dir(dir: &Path) -> bool {
    match fs::symlink_metadata(dir) {
        Ok(meta) if meta.is_dir() && !is_link_like(&meta) => fs::remove_dir(dir).is_ok(),
        _ => false,
    }
}

/// Clean matching files under `root` (caller re-validates the root first).
pub(crate) fn clean_root(
    root: &Path,
    spec: &WalkSpec<'_>,
    mode: CleanMode,
    now: SystemTime,
    on_removed: &mut dyn FnMut(&Path, u64),
) -> CleanTally {
    let mut run = CleanRun {
        spec,
        now,
        track_rel: spec.matcher.uses_path_segments(),
        tally: CleanTally::default(),
        on_removed,
    };
    let mut rel = Vec::new();
    run.clean_dir(root, 0, &mut rel);
    let mut tally = run.tally;
    if mode == CleanMode::WholeRoot && !tally.cancelled && prune_dir(root) {
        tally.dirs_removed += 1;
    }
    tally
}

/// Provider helper: validate `root` with `policy`, then measure every file under it.
pub fn measure_directory(
    root: &Path,
    safety: &SafetyContext,
    policy: RootPolicy,
    on_file: &mut dyn FnMut(&Path, u64),
) -> Result<MeasureTally, RootRefusal> {
    validate_root(root, safety, policy)?;
    let matcher = MatcherSpec::default();
    let excluded = HashSet::new();
    let spec = WalkSpec { matcher: &matcher, excluded: &excluded };
    Ok(measure_root(root, &spec, MAX_ENTRIES_PER_RULE, SystemTime::now(), on_file))
}

/// Provider helper: re-validate `root` with `policy`, then delete every file under
/// it (and the root itself in `WholeRoot` mode) with the same per-entry checks as
/// catalog rules.
pub fn clean_directory(
    root: &Path,
    mode: CleanMode,
    safety: &SafetyContext,
    policy: RootPolicy,
    on_removed: &mut dyn FnMut(&Path, u64),
) -> Result<CleanTally, RootRefusal> {
    validate_root(root, safety, policy)?;
    let matcher = MatcherSpec::default();
    let excluded = HashSet::new();
    let spec = WalkSpec { matcher: &matcher, excluded: &excluded };
    Ok(clean_root(root, &spec, mode, SystemTime::now(), on_removed))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::time::Duration;

    fn test_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("pctoolkit-walk-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    fn write_file(path: &Path, bytes: &[u8]) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::File::create(path).unwrap().write_all(bytes).unwrap();
    }

    fn set_mtime_days_ago(path: &Path, days: u64) {
        let file = fs::File::options().write(true).open(path).unwrap();
        file.set_modified(SystemTime::now() - Duration::from_secs(days * 86_400)).unwrap();
    }

    fn all() -> MatcherSpec {
        MatcherSpec::default()
    }

    fn measure(root: &Path, matcher: &MatcherSpec, excluded: &HashSet<String>) -> MeasureTally {
        let spec = WalkSpec { matcher, excluded };
        measure_root(root, &spec, MAX_ENTRIES_PER_RULE, SystemTime::now(), &mut |_, _| {})
    }

    fn clean(root: &Path, matcher: &MatcherSpec, mode: CleanMode) -> CleanTally {
        let excluded = HashSet::new();
        let spec = WalkSpec { matcher, excluded: &excluded };
        clean_root(root, &spec, mode, SystemTime::now(), &mut |_, _| {})
    }

    #[cfg(windows)]
    fn make_junction(link: &Path, target: &Path) -> bool {
        let mut cmd = std::process::Command::new(pctoolkit_platform::system32_exe("cmd.exe"));
        cmd.args(["/C", "mklink", "/J"]).arg(link).arg(target);
        pctoolkit_platform::hide_console(&mut cmd);
        cmd.output().map(|o| o.status.success()).unwrap_or(false)
    }

    #[cfg(windows)]
    #[test]
    fn junction_is_neither_measured_nor_cleaned_nor_followed() {
        let root = test_dir("junction-root");
        let outside = test_dir("junction-outside");
        write_file(&root.join("own.bin"), b"12345");
        write_file(&outside.join("precious.bin"), &[7u8; 2048]);
        write_file(&outside.join("nested").join("deep.bin"), &[1u8; 64]);
        let link = root.join("link-out");
        assert!(make_junction(&link, &outside), "mklink /J must succeed in a temp dir");

        let meta = fs::symlink_metadata(&link).unwrap();
        assert!(is_link_like(&meta), "junction must be detected as link-like");

        let measured = measure(&root, &all(), &HashSet::new());
        assert_eq!(measured.files, 1, "junction target must not be counted");
        assert_eq!(measured.bytes, 5);

        let tally = clean(&root, &all(), CleanMode::Contents);
        assert_eq!(tally.removed, 1);
        assert_eq!(tally.freed_bytes, 5);
        assert!(outside.join("precious.bin").exists(), "junction target must survive");
        assert!(outside.join("nested").join("deep.bin").exists());
        assert!(fs::symlink_metadata(&link).is_ok(), "the junction itself is never deleted");

        // A root whose path goes through the junction is refused outright.
        let through = link.join("nested");
        let refused = validate_root(&through, &SafetyContext::default(), RootPolicy::default());
        assert_eq!(refused, Err(RootRefusal::LinkComponent));
        assert!(clean_directory(&through, CleanMode::WholeRoot, &SafetyContext::default(), RootPolicy::default(), &mut |_, _| {}).is_err());
        assert!(outside.join("nested").join("deep.bin").exists());

        let _ = fs::remove_dir(&link);
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&outside);
    }

    #[test]
    fn symlinked_file_is_skipped_when_supported() {
        let root = test_dir("symlink-skip");
        let outside = test_dir("symlink-skip-outside");
        write_file(&root.join("real.bin"), b"hello");
        write_file(&outside.join("huge.bin"), &[0u8; 1024]);
        let link = root.join("link-out");
        #[cfg(windows)]
        let linked = std::os::windows::fs::symlink_file(outside.join("huge.bin"), &link).is_ok();
        #[cfg(unix)]
        let linked = std::os::unix::fs::symlink(outside.join("huge.bin"), &link).is_ok();
        if linked {
            let m = measure(&root, &all(), &HashSet::new());
            assert_eq!((m.files, m.bytes), (1, 5));
            clean(&root, &all(), CleanMode::Contents);
            assert!(outside.join("huge.bin").exists());
            assert!(fs::symlink_metadata(&link).is_ok(), "links are never deleted");
        }
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&outside);
    }

    #[test]
    fn filters_age_extension_and_depth_for_measure_and_clean() {
        let root = test_dir("filters");
        write_file(&root.join("old.crdownload"), b"aaaa");
        write_file(&root.join("fresh.crdownload"), b"bbbb");
        write_file(&root.join("keep.txt"), b"cccc");
        write_file(&root.join("nested").join("deep.partial"), b"dddd");
        let too_deep = root.join("a").join("b").join("c").join("d").join("too-deep.part");
        write_file(&too_deep, b"eeee");
        set_mtime_days_ago(&root.join("old.crdownload"), 10);
        set_mtime_days_ago(&root.join("fresh.crdownload"), 1);
        set_mtime_days_ago(&root.join("nested").join("deep.partial"), 10);
        set_mtime_days_ago(&too_deep, 10);
        fs::create_dir_all(root.join("pre-existing-empty")).unwrap();

        let mut matcher = MatcherSpec {
            extensions: vec!["crdownload".into(), "download".into(), "partial".into(), "part".into()],
            older_than_days: Some(7),
            max_depth: Some(3),
            ..MatcherSpec::default()
        };
        matcher.normalize();
        let m = measure(&root, &matcher, &HashSet::new());
        assert_eq!((m.files, m.bytes), (2, 8));

        let t = clean(&root, &matcher, CleanMode::Contents);
        assert_eq!((t.removed, t.freed_bytes), (2, 8));
        assert!(root.join("fresh.crdownload").exists());
        assert!(root.join("keep.txt").exists());
        assert!(!root.join("old.crdownload").exists());
        assert!(!root.join("nested").exists(), "folder emptied by the clean is pruned");
        assert!(root.join("pre-existing-empty").exists(), "untouched empty folders stay");
        assert!(too_deep.exists());
        assert!(root.exists(), "contents mode keeps the root");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn excluded_subtrees_are_skipped() {
        let root = test_dir("owned");
        write_file(&root.join("a.tmp"), b"1");
        write_file(&root.join("Low").join("b.tmp"), b"22");
        let excluded: HashSet<String> = [path_key(&root.join("Low"))].into_iter().collect();
        let m = measure(&root, &all(), &excluded);
        assert_eq!((m.files, m.bytes), (1, 1));
        let spec = WalkSpec { matcher: &all(), excluded: &excluded };
        let t = clean_root(&root, &spec, CleanMode::Contents, SystemTime::now(), &mut |_, _| {});
        assert_eq!(t.removed, 1);
        assert!(root.join("Low").join("b.tmp").exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn whole_root_mode_removes_the_emptied_root() {
        let base = test_dir("whole");
        let root = base.join("Cache");
        write_file(&root.join("x").join("y.bin"), b"abc");
        let t = clean(&root, &all(), CleanMode::WholeRoot);
        assert_eq!(t.removed, 1);
        assert!(!root.exists());
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn entry_budget_marks_limited() {
        let root = test_dir("limit");
        for i in 0..10 {
            write_file(&root.join(format!("f{i}.bin")), b"z");
        }
        let excluded = HashSet::new();
        let matcher = all();
        let spec = WalkSpec { matcher: &matcher, excluded: &excluded };
        let m = measure_root(&root, &spec, 4, SystemTime::now(), &mut |_, _| {});
        assert!(m.limited);
        assert_eq!(m.files, 4);
        let _ = fs::remove_dir_all(&root);
    }

    #[cfg(windows)]
    #[test]
    fn locked_file_counts_as_busy_and_others_continue() {
        use std::os::windows::fs::OpenOptionsExt;
        let root = test_dir("busy");
        write_file(&root.join("locked.bin"), b"locked");
        write_file(&root.join("free.bin"), b"free!");
        // Share mode 0: no other open (including delete) is allowed.
        let _guard = fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(root.join("locked.bin"))
            .unwrap();
        let t = clean(&root, &all(), CleanMode::Contents);
        assert_eq!(t.removed, 1);
        assert_eq!(t.freed_bytes, 5);
        assert_eq!(t.busy, 1);
        assert_eq!(t.failed, 0);
        drop(_guard);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn read_only_files_are_removed() {
        let root = test_dir("readonly");
        let file = root.join("ro.bin");
        write_file(&file, b"ro");
        let mut perms = fs::metadata(&file).unwrap().permissions();
        perms.set_readonly(true);
        fs::set_permissions(&file, perms).unwrap();
        let t = clean(&root, &all(), CleanMode::Contents);
        assert_eq!(t.removed, 1);
        assert!(!file.exists());
        let _ = fs::remove_dir_all(&root);
    }
}

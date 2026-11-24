use super::*;
use std::collections::HashSet;
use std::io::Write;
use std::sync::atomic::{AtomicU32, Ordering};

struct TempTree(PathBuf);

impl TempTree {
    fn new(tag: &str) -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let path = std::env::temp_dir().join(format!(
            "pctoolkit-projects-{tag}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn file(&self, rel: &str, bytes: usize) -> PathBuf {
        let path = self.0.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::File::create(&path).unwrap().write_all(&vec![1u8; bytes]).unwrap();
        path
    }

    fn dir(&self, rel: &str) -> PathBuf {
        let path = self.0.join(rel);
        fs::create_dir_all(&path).unwrap();
        path
    }
}

impl Drop for TempTree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn limits() -> DiscoveryLimits {
    DiscoveryLimits {
        time_budget: Duration::from_secs(20),
        ..DiscoveryLimits::default()
    }
}

fn run(root: &Path) -> Discovery {
    discover(&[root.to_path_buf()], limits(), &|_| {})
}

fn find<'a>(d: &'a Discovery, path: &Path) -> Option<&'a Project> {
    d.projects.iter().find(|p| path_key(&p.path) == path_key(path))
}

fn artifact_names_of(p: &Project) -> HashSet<String> {
    p.artifacts.iter().map(|a| relative_name(a, &p.path)).collect()
}

fn age(path: &Path, days: u64) {
    let file = fs::File::options().write(true).open(path).unwrap();
    file.set_modified(SystemTime::now() - Duration::from_secs(days * 86_400)).unwrap();
}

/// Create a directory junction without a console window. `false` when the
/// platform or environment cannot create one (the test then skips).
fn junction(link: &Path, target: &Path) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        // mklink parses '/' as a switch: pass backslash-only paths.
        let native = |p: &Path| p.to_string_lossy().replace('/', "\\");
        std::process::Command::new(pctoolkit_platform::system32_exe("cmd.exe"))
            .args(["/C", "mklink", "/J"])
            .arg(native(link))
            .arg(native(target))
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .creation_flags(CREATE_NO_WINDOW)
            .status()
            .is_ok_and(|s| s.success())
    }
    #[cfg(not(windows))]
    {
        let _ = (link, target);
        false
    }
}

#[test]
fn discovers_projects_by_marker_and_lists_their_artifacts() {
    let t = TempTree::new("discover");
    t.file("web/package.json", 2);
    t.file("web/node_modules/dep/index.js", 10);
    t.file("web/.next/cache/a.bin", 10);
    t.file("web/.next/server/keep.js", 1);
    t.file("tool/Cargo.toml", 2);
    t.file("tool/target/debug/app.exe", 10);
    t.file("game/Assets/a.txt", 1);
    t.dir("game/ProjectSettings");
    t.file("game/Library/cache.bin", 10);
    t.file("native/CMakeLists.txt", 1);
    t.file("native/cmake-build-debug/out.o", 4);
    t.file("app/App.csproj", 1);
    t.file("app/bin/Debug/app.dll", 4);
    t.file("app/obj/project.assets.json", 4);

    let d = run(&t.0);
    let web = find(&d, &t.0.join("web")).expect("node project");
    assert_eq!(web.ecosystems, vec!["node"]);
    assert_eq!(
        artifact_names_of(web),
        HashSet::from(["node_modules".to_string(), ".next/cache".to_string()])
    );
    let tool = find(&d, &t.0.join("tool")).expect("rust project");
    assert_eq!(artifact_names_of(tool), HashSet::from(["target".to_string()]));
    let game = find(&d, &t.0.join("game")).expect("unity project");
    assert!(artifact_names_of(game).contains("Library"));
    let native = find(&d, &t.0.join("native")).expect("cmake project");
    assert!(artifact_names_of(native).contains("cmake-build-debug"));
    let app = find(&d, &t.0.join("app")).expect("dotnet project");
    assert_eq!(artifact_names_of(app), HashSet::from(["bin".to_string(), "obj".to_string()]));
    assert!(!d.limited);
}

#[test]
fn never_descends_into_dependencies_hidden_or_vcs_folders() {
    let t = TempTree::new("skip");
    t.file("web/package.json", 1);
    // Markers inside dependency / hidden / VCS / venv folders are not projects.
    t.file("web/node_modules/dep/package.json", 1);
    t.file("web/node_modules/dep/node_modules/x/a.js", 1);
    t.file(".hidden/proj/Cargo.toml", 1);
    t.file("repo/.git/sub/Cargo.toml", 1);
    t.file("env/pyvenv.cfg", 1);
    t.file("env/Lib/site-packages/pkg/setup.py", 1);

    let d = run(&t.0);
    let paths: Vec<String> = d.projects.iter().map(|p| relative_name(&p.path, &t.0)).collect();
    assert_eq!(paths, vec!["web".to_string()], "{paths:?}");
}

#[test]
fn marker_missing_means_no_project_and_marker_must_be_in_the_same_folder() {
    let t = TempTree::new("marker");
    t.file("loose/node_modules/a.js", 4);
    t.file("loose/target/debug/a.exe", 4);
    t.file("nested/sub/package.json", 1);
    t.file("nested/node_modules/a.js", 4);

    let d = run(&t.0);
    assert!(find(&d, &t.0.join("loose")).is_none());
    assert!(find(&d, &t.0.join("nested")).is_none());
    let sub = find(&d, &t.0.join("nested/sub")).expect("marker folder is the project");
    assert!(sub.artifacts.is_empty(), "the parent's node_modules is not the sub project's");
}

#[test]
fn nested_artifacts_keep_the_shallowest() {
    let t = TempTree::new("nested");
    t.file("py/pyproject.toml", 1);
    t.file("py/pkg/__pycache__/m.pyc", 3);
    t.file("py/pkg/deep/__pycache__/n.pyc", 3);
    t.file("py/.tox/env/__pycache__/x.pyc", 3);

    let d = run(&t.0);
    let py = find(&d, &t.0.join("py")).expect("python project");
    let names = artifact_names_of(py);
    assert!(names.contains(".tox"));
    assert!(names.contains("pkg/__pycache__"));
    assert!(names.contains("pkg/deep/__pycache__"));
    assert!(!names.iter().any(|n| n.starts_with(".tox/")), "{names:?}");

    // The pure pass drops any artifact inside another artifact.
    let mut projects = vec![Project {
        path: t.0.join("a"),
        ecosystems: vec!["node"],
        artifacts: vec![t.0.join("a/build"), t.0.join("a/build/inner"), t.0.join("a/obj")],
        newest_mtime: None,
        parent: None,
        pycache_ok: true,
    }];
    keep_shallowest(&mut projects);
    assert_eq!(projects[0].artifacts, vec![t.0.join("a/build"), t.0.join("a/obj")]);
}

#[test]
fn recent_sources_mark_a_project_active() {
    let t = TempTree::new("active");
    let old_marker = t.file("old/package.json", 1);
    let old_src = t.file("old/src/index.js", 1);
    t.file("old/node_modules/a.js", 1);
    age(&old_marker, 30);
    age(&old_src, 30);
    t.file("new/package.json", 1);
    t.file("new/node_modules/a.js", 1);

    let d = run(&t.0);
    let now = SystemTime::now();
    let old = find(&d, &t.0.join("old")).unwrap();
    let new = find(&d, &t.0.join("new")).unwrap();
    assert!(!is_active(old.newest_mtime, now), "only artifact files are recent");
    assert!(is_active(new.newest_mtime, now));
    assert!(!is_active(None, now));
}

#[test]
fn junctions_are_never_followed_or_offered() {
    let t = TempTree::new("junction");
    let outside = TempTree::new("junction-target");
    outside.file("victim/package.json", 1);
    outside.file("victim/node_modules/keep.js", 5);
    t.file("web/package.json", 1);
    if !junction(&t.0.join("web/node_modules"), &outside.0.join("victim/node_modules"))
        || !junction(&t.0.join("linked"), &outside.0.join("victim"))
    {
        eprintln!("junctions unavailable; skipping");
        return;
    }
    let d = run(&t.0);
    let web = find(&d, &t.0.join("web")).unwrap();
    assert!(web.artifacts.is_empty(), "a junction artifact is not a real directory");
    assert!(find(&d, &t.0.join("linked")).is_none(), "junctioned folders are not walked");
    let _ = fs::remove_dir(t.0.join("web/node_modules"));
    let _ = fs::remove_dir(t.0.join("linked"));
    assert!(outside.0.join("victim/node_modules/keep.js").exists());
}

#[test]
fn gitignore_and_embedded_repositories_guard_tracked_folders() {
    let t = TempTree::new("git");
    // Repository root whose .gitignore does not mention `build`: possibly tracked.
    t.file("lib/build.gradle", 1);
    t.dir("lib/.git");
    t.file("lib/.gitignore", 1);
    fs::write(t.0.join("lib/.gitignore"), "/.gradle/\n*.log\n").unwrap();
    t.file("lib/build/tracked.txt", 1);
    t.file("lib/.gradle/cache.bin", 1);
    // Same, but ignored: offered. `[Bb]in` style classes are understood.
    t.file("cs/App.csproj", 1);
    t.dir("cs/.git");
    fs::write(t.0.join("cs/.gitignore"), "[Bb]in/\n[Oo]bj/\n").unwrap();
    t.file("cs/bin/a.dll", 1);
    t.file("cs/obj/a.json", 1);
    // An artifact that is itself a repository is never offered.
    t.file("node/package.json", 1);
    t.dir("node/node_modules/.git");

    let d = run(&t.0);
    let lib = find(&d, &t.0.join("lib")).unwrap();
    assert_eq!(artifact_names_of(lib), HashSet::from([".gradle".to_string()]));
    let cs = find(&d, &t.0.join("cs")).unwrap();
    assert_eq!(artifact_names_of(cs), HashSet::from(["bin".to_string(), "obj".to_string()]));
    let node = find(&d, &t.0.join("node")).unwrap();
    assert!(node.artifacts.is_empty());
}

#[test]
fn discovery_is_bounded_and_reports_limited() {
    let t = TempTree::new("limit");
    for i in 0..5 {
        t.file(&format!("p{i}/package.json"), 1);
    }
    let tight = DiscoveryLimits {
        max_projects: 2,
        ..limits()
    };
    let d = discover(&[t.0.clone()], tight, &|_| {});
    assert!(d.limited);
    assert!(d.projects.len() <= 2);
}

#[test]
fn discovery_roots_skip_appdata_hidden_media_and_onedrive() {
    let home = TempTree::new("home");
    for name in ["AppData", ".config", "Pictures", "OneDrive - Contoso", "code", "Documents"] {
        home.dir(name);
    }
    let drive = TempTree::new("drive");
    drive.dir("code");
    drive.dir("Games");
    let roots = discovery_roots(Some(&home.0), &[drive.0.clone()]);
    let names: Vec<String> = roots
        .iter()
        .map(|r| r.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, vec!["code".to_string(), "Documents".to_string(), "code".to_string()]);
}

#[test]
fn extra_drive_roots_skip_system_program_and_game_folders() {
    let drive = TempTree::new("extra-drive");
    for name in [
        "Windows",
        "Program Files",
        "ProgramData",
        "Users",
        "$Recycle.Bin",
        "SteamLibrary",
        ".hidden",
        "work",
        "Clients",
    ] {
        drive.dir(name);
    }
    let roots = extra_drive_roots(&[drive.0.clone()]);
    let names: Vec<String> = roots
        .iter()
        .map(|r| r.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, vec!["Clients".to_string(), "work".to_string()]);
}

#[test]
fn ids_are_stable_and_path_free() {
    let a = project_id(Path::new(r"C:\Users\Tester\code\App"));
    let b = project_id(Path::new(r"c:\users\tester\CODE\app"));
    assert_eq!(a, b);
    assert!(a.starts_with("projects."));
    assert!(!a.contains('\\') && !a.to_lowercase().contains("tester"));
    assert_eq!(
        path_hint(Path::new(r"C:\Users\Tester\code\App"), Some(Path::new(r"C:\Users\Tester"))),
        r"~\code\App"
    );
}

#[test]
fn execute_reconfirms_marker_and_cleans_only_artifacts() {
    let t = TempTree::new("exec");
    let safety = SafetyContext::default();
    let project = t.0.join("web");
    t.file("web/package.json", 1);
    t.file("web/src/index.js", 3);
    t.file("web/node_modules/dep/a.js", 7);
    t.file("web/node_modules/dep/b.js", 5);
    let id = project_id(&project);

    let done = clean_project(&id, &project, &safety);
    assert_eq!(done.status, DeepCleanupExecStatus::Completed, "{done:?}");
    assert_eq!((done.removed_count, done.freed_bytes), (2, 12));
    assert!(!project.join("node_modules").exists(), "whole artifact removed");
    assert!(project.join("src/index.js").exists() && project.join("package.json").exists());

    // Marker removed after the scan: nothing is deleted.
    t.file("web/node_modules/dep/a.js", 7);
    fs::remove_file(project.join("package.json")).unwrap();
    let changed = clean_project(&id, &project, &safety);
    assert_eq!(changed.status, DeepCleanupExecStatus::Skipped);
    assert_eq!(changed.reason, Some(DeepCleanupExecReason::ChangedSinceScan));
    assert!(project.join("node_modules/dep/a.js").exists());

    // An id that does not belong to the path is refused.
    t.file("web/package.json", 1);
    let forged = clean_project("projects.0000000000000000", &project, &safety);
    assert_eq!(forged.reason, Some(DeepCleanupExecReason::ChangedSinceScan));
    assert!(project.join("node_modules/dep/a.js").exists());
}

#[test]
fn provider_execute_uses_only_core_held_paths() {
    let t = TempTree::new("provider");
    t.file("web/package.json", 1);
    t.file("web/node_modules/a.js", 2);
    let project = t.0.join("web");
    let id = project_id(&project);
    let running = HashSet::new();
    let safety = SafetyContext::default();
    let progress = |_: &str, _: &Path, _: u64, _: u64| {};
    let ctx = ProviderContext::new(false, &running, &safety, &progress);

    let unknown = ProjectsProvider.execute(&["projects.ffffffffffffffff".to_string()], &ctx).unwrap();
    assert_eq!(unknown[0].reason, Some(DeepCleanupExecReason::NotApplicable));

    remember_for_test(&id, &project);
    let done = ProjectsProvider.execute(std::slice::from_ref(&id), &ctx).unwrap();
    assert_eq!(done[0].status, DeepCleanupExecStatus::Completed);
    assert!(!project.join("node_modules").exists());
}

#[test]
fn scan_results_are_never_preselected_and_flag_active_work() {
    let t = TempTree::new("results");
    t.file("web/package.json", 1);
    t.file("web/node_modules/a.js", 9);
    let running = HashSet::new();
    let safety = SafetyContext::default();
    let progress = |_: &str, _: &Path, _: u64, _: u64| {};
    let ctx = ProviderContext::new(false, &running, &safety, &progress);
    let d = run(&t.0);
    let results = scan_projects(&d, &ctx, Some(&t.0));
    assert_eq!(results.len(), 1);
    let r = &results[0].0;
    assert_eq!((r.bytes, r.item_count), (9, 1));
    assert_eq!(r.group, DeepCleanupGroup::Projects);
    assert!(!r.recommended && !r.selected);
    assert!(r.extras.active, "just created");
    assert_eq!(r.extras.display_name.as_deref(), Some("web"));
    assert_eq!(r.extras.path_hint.as_deref(), Some(r"~\web"));
    assert_eq!(r.extras.artifacts, vec!["node_modules".to_string()]);
    assert_eq!(r.extras.tags, vec!["node".to_string()]);
}

/// Read-only discovery + measurement on this machine. Never cleans.
/// `cargo test -p pctoolkit-core -- --ignored projects_smoke --nocapture`
#[test]
#[ignore]
fn projects_smoke_on_this_machine() {
    let started = Instant::now();
    let roots = default_roots();
    let d = discover(&roots, DiscoveryLimits::default(), &|_| {});
    let discovered = started.elapsed();
    let running = HashSet::new();
    let safety = SafetyContext::from_env();
    let progress = |_: &str, _: &Path, _: u64, _: u64| {};
    let ctx = ProviderContext::new(false, &running, &safety, &progress);
    let home = home_dir();
    let results = scan_projects(&d, &ctx, home.as_deref());
    println!(
        "roots={} projects={} withData={} limited={} discoverMs={} totalMs={}",
        roots.len(),
        d.projects.len(),
        results.len(),
        d.limited,
        discovered.as_millis(),
        started.elapsed().as_millis()
    );
    for (r, _) in results.iter().take(5) {
        println!(
            "  {:>12} {} [{}] {:?} active={}",
            r.bytes,
            r.extras.path_hint.as_deref().unwrap_or(""),
            r.extras.tags.join(","),
            r.extras.artifacts,
            r.extras.active
        );
    }
}

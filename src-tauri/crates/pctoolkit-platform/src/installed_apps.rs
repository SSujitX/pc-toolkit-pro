//! Installed program facts: Win32 "Uninstall" registry entries and per-user Store (MSIX/AppX)
//! packages. This module reports what Windows says is installed; filtering and product decisions
//! (what to show, what is safe to remove) belong to Core.

use crate::registry::{self, RegistryKeyPath, RegistryRoot, RegistryView};
use crate::{PlatformError, PlatformResult};

pub const UNINSTALL_SUBKEY: &str = "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall";

/// One raw entry under an `...\CurrentVersion\Uninstall` key.
#[derive(Debug, Clone, Default)]
pub struct UninstallEntry {
    pub key: Option<RegistryKeyPath>,
    pub key_name: String,
    pub display_name: String,
    pub display_version: String,
    pub publisher: String,
    /// `YYYYMMDD` as written by most installers; empty when unknown.
    pub install_date: String,
    pub install_location: String,
    pub display_icon: String,
    pub estimated_size_kb: u64,
    pub uninstall_string: String,
    pub quiet_uninstall_string: String,
    pub system_component: bool,
    pub windows_installer: bool,
    pub no_remove: bool,
    pub parent_key_name: String,
    pub release_type: String,
    /// Uninstall key last write time (Unix ms); a fallback install date. 0 when unknown.
    pub key_written_ms: i64,
}

/// The four registry views that hold Win32 uninstall entries. HKCU is not redirected for
/// 32-bit software on 64-bit Windows, so it is read once.
pub fn uninstall_roots() -> Vec<RegistryKeyPath> {
    vec![
        RegistryKeyPath::new(RegistryRoot::LocalMachine, RegistryView::Native64, UNINSTALL_SUBKEY),
        RegistryKeyPath::new(RegistryRoot::LocalMachine, RegistryView::Wow32, UNINSTALL_SUBKEY),
        RegistryKeyPath::new(RegistryRoot::CurrentUser, RegistryView::Native64, UNINSTALL_SUBKEY),
    ]
}

fn flag(path: &RegistryKeyPath, name: &str) -> bool {
    registry::read_value(path, name).is_some_and(|v| v.trim() == "1")
}

fn text(path: &RegistryKeyPath, name: &str) -> String {
    registry::read_value(path, name).unwrap_or_default()
}

/// Read one uninstall entry. Returns `None` when the key disappeared between enumeration and read.
pub fn read_uninstall_entry(key: &RegistryKeyPath) -> Option<UninstallEntry> {
    if !registry::key_exists(key) {
        return None;
    }
    let key_name = key
        .subkey
        .rsplit('\\')
        .next()
        .unwrap_or_default()
        .to_string();
    Some(UninstallEntry {
        key: Some(key.clone()),
        key_name,
        display_name: text(key, "DisplayName"),
        display_version: text(key, "DisplayVersion"),
        publisher: text(key, "Publisher"),
        install_date: text(key, "InstallDate"),
        install_location: text(key, "InstallLocation"),
        display_icon: text(key, "DisplayIcon"),
        estimated_size_kb: text(key, "EstimatedSize").parse().unwrap_or(0),
        uninstall_string: text(key, "UninstallString"),
        quiet_uninstall_string: text(key, "QuietUninstallString"),
        system_component: flag(key, "SystemComponent"),
        windows_installer: flag(key, "WindowsInstaller"),
        no_remove: flag(key, "NoRemove"),
        parent_key_name: text(key, "ParentKeyName"),
        release_type: text(key, "ReleaseType"),
        key_written_ms: registry::key_last_write_ms(key).unwrap_or(0),
    })
}

/// Enumerate every uninstall entry in all registry views. Unreadable views are skipped.
pub fn list_uninstall_entries() -> Vec<UninstallEntry> {
    let mut out = Vec::new();
    for root in uninstall_roots() {
        let Ok(names) = registry::subkey_names(&root) else {
            continue;
        };
        for name in names {
            if let Some(entry) = read_uninstall_entry(&root.child(&name)) {
                out.push(entry);
            }
        }
    }
    out
}

/// A per-user Store (MSIX/AppX) package.
#[derive(Debug, Clone, Default)]
pub struct StorePackage {
    pub full_name: String,
    pub family_name: String,
    pub display_name: String,
    pub publisher: String,
    pub version: String,
    pub install_path: String,
    /// Unix milliseconds; 0 when unknown.
    pub installed_at_ms: i64,
    pub is_framework: bool,
    pub is_resource: bool,
    /// Signed by Windows itself (inbox app) — removal is usually blocked or unsafe.
    pub is_system: bool,
    pub is_development_mode: bool,
    /// Absolute path of the package logo image (best scale variant on disk); empty when unknown.
    pub logo_path: String,
}

/// Percent-decode a `file:///C:/...` URI into a Windows path.
#[cfg_attr(not(windows), allow(dead_code))]
fn file_uri_to_path(uri: &str) -> Option<std::path::PathBuf> {
    let rest = uri.strip_prefix("file:///")?;
    let bytes = rest.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(v) = u8::from_str_radix(&rest[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(if bytes[i] == b'/' { b'\\' } else { bytes[i] });
        i += 1;
    }
    String::from_utf8(out).ok().map(std::path::PathBuf::from)
}

/// Store manifests name `Assets\StoreLogo.png`, but packages ship scale / target-size variants
/// (`StoreLogo.scale-200.png`, `StoreLogo.targetsize-48.png`). Pick the one closest to ~48 px.
pub fn resolve_store_logo(declared: &std::path::Path) -> Option<std::path::PathBuf> {
    if declared.is_file() {
        return Some(declared.to_path_buf());
    }
    let dir = declared.parent()?;
    let stem = declared.file_stem()?.to_string_lossy().to_lowercase();
    let mut best: Option<(u32, std::path::PathBuf)> = None;
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let name = entry.file_name().to_string_lossy().to_lowercase();
        if !name.starts_with(&format!("{stem}.")) || !name.ends_with(".png") {
            continue;
        }
        let score = if name.contains("contrast") {
            900
        } else if name.contains("targetsize-48") {
            0
        } else if name.contains("scale-200") {
            10
        } else if name.contains("scale-150") {
            20
        } else if name.contains("scale-100") {
            30
        } else {
            100
        };
        if best.as_ref().is_none_or(|(current, _)| score < *current) {
            best = Some((score, entry.path()));
        }
    }
    best.map(|(_, p)| p)
}

#[cfg(windows)]
pub fn list_store_packages() -> PlatformResult<Vec<StorePackage>> {
    use windows::core::HSTRING;
    use windows::ApplicationModel::PackageSignatureKind;
    use windows::Management::Deployment::PackageManager;

    let manager = PackageManager::new()
        .map_err(|e| PlatformError::OperationFailed(format!("packageManager:{:?}", e.code())))?;
    // Empty SID = packages registered for the current user (no admin required).
    let packages = manager
        .FindPackagesByUserSecurityId(&HSTRING::new())
        .map_err(|e| PlatformError::OperationFailed(format!("findPackages:{:?}", e.code())))?;

    let mut out = Vec::new();
    for package in packages {
        let Ok(id) = package.Id() else { continue };
        let version = id
            .Version()
            .map(|v| format!("{}.{}.{}.{}", v.Major, v.Minor, v.Build, v.Revision))
            .unwrap_or_default();
        let signature = package.SignatureKind().ok();
        // WinRT DateTime = 100ns ticks since 1601-01-01.
        let installed_at_ms = package
            .InstalledDate()
            .map(|d| (d.UniversalTime - 116_444_736_000_000_000) / 10_000)
            .unwrap_or(0);
        out.push(StorePackage {
            full_name: id.FullName().map(|s| s.to_string()).unwrap_or_default(),
            family_name: id.FamilyName().map(|s| s.to_string()).unwrap_or_default(),
            display_name: package.DisplayName().map(|s| s.to_string()).unwrap_or_default(),
            publisher: package
                .PublisherDisplayName()
                .map(|s| s.to_string())
                .unwrap_or_default(),
            version,
            install_path: package.InstalledPath().map(|s| s.to_string()).unwrap_or_default(),
            installed_at_ms: installed_at_ms.max(0),
            is_framework: package.IsFramework().unwrap_or(false),
            is_resource: package.IsResourcePackage().unwrap_or(false),
            is_system: signature == Some(PackageSignatureKind::System),
            is_development_mode: package.IsDevelopmentMode().unwrap_or(false),
            logo_path: package
                .Logo()
                .and_then(|uri| uri.AbsoluteUri())
                .ok()
                .and_then(|uri| file_uri_to_path(&uri.to_string()))
                .and_then(|p| resolve_store_logo(&p))
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default(),
        });
    }
    Ok(out)
}

#[cfg(not(windows))]
pub fn list_store_packages() -> PlatformResult<Vec<StorePackage>> {
    Err(PlatformError::Unsupported)
}

/// Remove a Store package for the current user and wait for the deployment to finish.
#[cfg(windows)]
pub fn remove_store_package(full_name: &str) -> PlatformResult<()> {
    use windows::core::HSTRING;
    use windows::Management::Deployment::PackageManager;

    let manager = PackageManager::new()
        .map_err(|e| PlatformError::OperationFailed(format!("packageManager:{:?}", e.code())))?;
    let operation = manager
        .RemovePackageAsync(&HSTRING::from(full_name))
        .map_err(|e| PlatformError::OperationFailed(format!("removePackage:{:?}", e.code())))?;
    let result = operation
        .get()
        .map_err(|e| PlatformError::OperationFailed(format!("removePackageWait:{:?}", e.code())))?;
    let code = result.ExtendedErrorCode().map(|c| c.0).unwrap_or(0);
    if code == 0 {
        Ok(())
    } else if code as u32 == 0x8007_0005 {
        Err(PlatformError::PermissionDenied)
    } else {
        Err(PlatformError::OperationFailed(format!("removePackageResult:{code:#x}")))
    }
}

#[cfg(not(windows))]
pub fn remove_store_package(_full_name: &str) -> PlatformResult<()> {
    Err(PlatformError::Unsupported)
}

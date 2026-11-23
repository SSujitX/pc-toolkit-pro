//! Program icons for the Uninstaller list, resolved the way Explorer / Settings do: the
//! `DisplayIcon` resource, else the program's main executable, else the uninstaller itself.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use pctoolkit_platform::{
    icon_png_from_file, read_registry_value, RegistryKeyPath, RegistryRoot, RegistryView,
};
use serde::Serialize;

use super::inventory::{msi_product_code, ProgramRecord};
use super::matching::{compact, first_path};

const ICON_SIZE: u32 = 32;
/// Store logos larger than this are skipped rather than shipped as huge data URLs.
const MAX_LOGO_BYTES: u64 = 256 * 1024;

static CACHE: Mutex<Option<HashMap<String, Option<String>>>> = Mutex::new(None);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgramIcon {
    pub id: String,
    /// `data:image/png;base64,...`, or `None` when the program has no usable icon.
    pub data_url: Option<String>,
}

/// Split `DisplayIcon` values such as `"C:\App\app.exe",0`, `C:\App\app.ico`, or
/// `C:\Windows\Installer\{GUID}\icon.exe,-101` into path + icon index.
pub(crate) fn parse_display_icon(value: &str) -> Option<(PathBuf, i32)> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    let (path_part, index) = match trimmed.rfind(',') {
        Some(pos) if trimmed[pos + 1..].trim().parse::<i32>().is_ok() => (
            &trimmed[..pos],
            trimmed[pos + 1..].trim().parse::<i32>().unwrap_or(0),
        ),
        _ => (trimmed, 0),
    };
    let path = path_part.trim().trim_matches('"');
    let expanded = expand_env(path);
    let bytes = expanded.as_bytes();
    (bytes.len() > 3 && bytes[1] == b':').then(|| (PathBuf::from(expanded), index))
}

fn expand_env(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('%') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after.find('%') {
            Some(end) => {
                let name = &after[..end];
                match std::env::var(name) {
                    Ok(value) => out.push_str(&value),
                    Err(_) => {
                        out.push('%');
                        out.push_str(name);
                        out.push('%');
                    }
                }
                rest = &after[end + 1..];
            }
            None => {
                out.push('%');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Windows Installer stores product data under a "packed" GUID: the first three groups are
/// reversed character-wise and every byte of the last two groups has its nibbles swapped.
pub(crate) fn packed_msi_guid(code: &str) -> Option<String> {
    let hex: String = code.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    if hex.len() != 32 {
        return None;
    }
    let mut out = String::with_capacity(32);
    out.extend(hex[0..8].chars().rev());
    out.extend(hex[8..12].chars().rev());
    out.extend(hex[12..16].chars().rev());
    for pair in hex.as_bytes()[16..].chunks(2) {
        out.push(pair[1] as char);
        out.push(pair[0] as char);
    }
    Some(out.to_uppercase())
}

/// `ARPPRODUCTICON` cached by Windows Installer (usually `C:\Windows\Installer\{GUID}\*.ico`).
fn msi_product_icon(code: &str) -> Option<PathBuf> {
    let packed = packed_msi_guid(code)?;
    [
        RegistryKeyPath::new(
            RegistryRoot::LocalMachine,
            RegistryView::Native64,
            format!("SOFTWARE\\Classes\\Installer\\Products\\{packed}"),
        ),
        RegistryKeyPath::new(
            RegistryRoot::CurrentUser,
            RegistryView::Native64,
            format!("Software\\Microsoft\\Installer\\Products\\{packed}"),
        ),
    ]
    .iter()
    .find_map(|key| read_registry_value(key, "ProductIcon"))
    .and_then(|value| parse_display_icon(&value))
    .map(|(path, _)| path)
}

/// The best executable in the install folder: one named like the program, else the largest.
fn main_executable(dir: &Path, name: &str) -> Option<PathBuf> {
    let wanted = compact(name);
    let mut best: Option<(u64, PathBuf)> = None;
    for entry in fs::read_dir(dir).ok()?.flatten() {
        let path = entry.path();
        let is_exe = path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("exe"));
        if !is_exe {
            continue;
        }
        let stem = path
            .file_stem()
            .map(|s| s.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        if stem.starts_with("unins") || stem.contains("uninst") || stem.contains("update") {
            continue;
        }
        let compact_stem = compact(&stem);
        if !compact_stem.is_empty() && (wanted.starts_with(&compact_stem) || compact_stem.starts_with(&wanted)) {
            return Some(path);
        }
        let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
        if best.as_ref().is_none_or(|(current, _)| size > *current) {
            best = Some((size, path));
        }
    }
    best.map(|(_, p)| p)
}

fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(TABLE[(n >> 18) as usize & 63] as char);
        out.push(TABLE[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { TABLE[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { TABLE[n as usize & 63] as char } else { '=' });
    }
    out
}

fn png_data_url(png: &[u8]) -> String {
    format!("data:image/png;base64,{}", base64(png))
}

fn resolve_icon(record: &ProgramRecord) -> Option<String> {
    if let Some(logo) = &record.store_logo {
        let small = fs::metadata(logo).map(|m| m.len() <= MAX_LOGO_BYTES).unwrap_or(false);
        if small {
            if let Ok(bytes) = fs::read(logo) {
                return Some(png_data_url(&bytes));
            }
        }
        return None;
    }
    let entry = record.entry.as_ref()?;
    let mut candidates: Vec<(PathBuf, i32)> = Vec::new();
    if let Some(icon) = parse_display_icon(&entry.display_icon) {
        candidates.push(icon);
    }
    if let Some(icon) = msi_product_code(entry).and_then(|code| msi_product_icon(&code)) {
        candidates.push((icon, 0));
    }
    if let Some(exe) = record
        .install_dir
        .as_ref()
        .and_then(|dir| main_executable(dir, &record.program.name))
    {
        candidates.push((exe, 0));
    }
    if let Some(uninstaller) = first_path(&entry.uninstall_string) {
        candidates.push((uninstaller, 0));
    }
    candidates
        .into_iter()
        .filter(|(path, _)| path.is_file())
        .find_map(|(path, index)| icon_png_from_file(&path, index, ICON_SIZE))
        .map(|png| png_data_url(&png))
}

/// Icon for one record through the shared cache (used by the listing feed, so the program list
/// later reuses the same result instead of extracting it again).
pub(crate) fn cached_icon(record: &ProgramRecord) -> Option<String> {
    let id = &record.program.id;
    if let Some(value) = CACHE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .and_then(|c| c.get(id).cloned())
    {
        return value;
    }
    let value = resolve_icon(record);
    CACHE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get_or_insert_with(HashMap::new)
        .insert(id.clone(), value.clone());
    value
}

/// Icons for the requested programs, using a process-wide cache.
pub(crate) fn program_icons(records: &[ProgramRecord], ids: &[String]) -> Vec<ProgramIcon> {
    let mut out = Vec::with_capacity(ids.len());
    for id in ids {
        let cached = CACHE
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .and_then(|c| c.get(id).cloned());
        let data_url = match cached {
            Some(value) => value,
            None => {
                let value = records
                    .iter()
                    .find(|r| &r.program.id == id)
                    .and_then(resolve_icon);
                CACHE
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .get_or_insert_with(HashMap::new)
                    .insert(id.clone(), value.clone());
                value
            }
        };
        out.push(ProgramIcon {
            id: id.clone(),
            data_url,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_display_icon_forms() {
        assert_eq!(
            parse_display_icon("\"C:\\Program Files\\App\\app.exe\",0"),
            Some((PathBuf::from("C:\\Program Files\\App\\app.exe"), 0))
        );
        assert_eq!(
            parse_display_icon("C:\\Windows\\Installer\\{X}\\icon.exe,-101"),
            Some((PathBuf::from("C:\\Windows\\Installer\\{X}\\icon.exe"), -101))
        );
        assert_eq!(
            parse_display_icon("C:\\App\\app.ico"),
            Some((PathBuf::from("C:\\App\\app.ico"), 0))
        );
        assert_eq!(parse_display_icon(""), None);
        assert_eq!(parse_display_icon("app.exe"), None);
    }

    #[test]
    fn packs_msi_product_codes() {
        assert_eq!(
            packed_msi_guid("{12345678-ABCD-EF01-2345-6789ABCDEF01}").as_deref(),
            Some("87654321DCBA10FE32547698BADCFE10")
        );
        assert_eq!(packed_msi_guid("not-a-guid"), None);
    }

    #[test]
    fn base64_matches_known_vectors() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }
}

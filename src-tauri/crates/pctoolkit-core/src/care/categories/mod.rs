//! Care category implementations, one file each. The engine runs them in this order for display;
//! independent categories scan in parallel.

pub mod privacy;
pub mod junk;
pub mod shortcuts;
pub mod registry;
pub mod startup;
pub mod security;
pub mod protection;
pub mod windows_update;
pub mod software;
pub mod disk_health;
pub mod disk_optimize;

use super::CareCategory;

/// Every Care category, in display order (matches `CareCategoryId::ALL`).
pub fn all() -> Vec<Box<dyn CareCategory>> {
    vec![
        Box::new(privacy::PrivacyCategory),
        Box::new(junk::JunkCategory),
        Box::new(shortcuts::ShortcutsCategory),
        Box::new(registry::RegistryCategory),
        Box::new(startup::StartupCategory),
        Box::new(security::SecurityCategory),
        Box::new(protection::ProtectionCategory),
        Box::new(windows_update::WindowsUpdateCategory),
        Box::new(software::SoftwareCategory),
        Box::new(disk_health::DiskHealthCategory),
        Box::new(disk_optimize::DiskOptimizeCategory),
    ]
}

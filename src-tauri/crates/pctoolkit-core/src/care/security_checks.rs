//! Security hardening checklist used by the Care **Security** category.
//!
//! Each check is data: one registry value, what counts as compliant, what an absent value means
//! (Windows' documented default), and an optional fix (write the recommended value, or delete a
//! policy value to hand control back to Windows). Only checks whose current value can be read and
//! whose fix can be reverted exactly are listed; anything else is informational (`fix: None`).
//!
//! Evaluation is pure over a [`SettingReader`], so the table is unit-tested with fake readers.

use pctoolkit_platform::registry_settings::RegistryData;
use pctoolkit_platform::{RegistryKeyPath, RegistryRoot, RegistryView};

use super::CareRisk;

/// Facts a check may depend on besides registry values (resolved once per scan).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecurityFact {
    /// Microsoft Defender Antivirus is the active (on) antivirus.
    DefenderActive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SettingRef {
    pub root: RegistryRoot,
    pub subkey: &'static str,
    pub name: &'static str,
}

impl SettingRef {
    pub fn key_path(&self) -> RegistryKeyPath {
        RegistryKeyPath::new(self.root, RegistryView::Native64, self.subkey)
    }

    /// Display-only location (`HKEY_…\key\value`).
    pub fn display(&self) -> String {
        format!("{}\\{}", self.key_path().display(), self.name)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Expect {
    DwordEq(u32),
    DwordNe(u32),
    DwordAtLeast(u32),
    DwordAtMost(u32),
    /// Every bit of the mask is set.
    DwordAllBits(u32),
    /// String value not equal (ASCII case-insensitive).
    StrNe(&'static str),
    StrEq(&'static str),
    /// The value must not exist.
    Absent,
}

/// How a missing value is interpreted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WhenAbsent {
    /// Windows behaves as if this DWORD were set.
    Dword(u32),
    Str(&'static str),
    Compliant,
    NonCompliant,
    /// Cannot tell — the check is not reported.
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cond {
    /// The value exists and matches (absent never matches, except for `Expect::Absent`).
    Value(SettingRef, Expect),
    KeyExists(RegistryRoot, &'static str),
    Fact(SecurityFact),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Desired {
    Dword(u32),
    Str(&'static str),
}

impl Desired {
    pub fn to_data(self) -> RegistryData {
        match self {
            Self::Dword(v) => RegistryData::Dword(v),
            Self::Str(s) => RegistryData::String(s.to_string()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecurityFix {
    Write(Desired),
    /// Delete the (policy) value so Windows' default / user setting applies again.
    Delete,
}

#[derive(Debug, Clone, Copy)]
pub struct SecurityCheck {
    /// Stable id: item id and locale key `care.security.checks.<id>`.
    pub id: &'static str,
    pub setting: SettingRef,
    pub expect: Expect,
    pub absent: WhenAbsent,
    /// `None` = informational (not fixable from here).
    pub fix: Option<SecurityFix>,
    pub risk: CareRisk,
    pub needs_admin: bool,
    /// All must hold for the check to apply.
    pub applies: &'static [Cond],
    /// Any holding makes the check compliant regardless of `setting` (e.g. a policy override).
    pub also_compliant: &'static [Cond],
    pub action: Option<&'static str>,
}

pub trait SettingReader {
    /// `Ok(None)` = absent; `Err(())` = unreadable (access denied, …).
    fn read(&self, setting: &SettingRef) -> Result<Option<RegistryData>, ()>;
    fn key_exists(&self, root: RegistryRoot, subkey: &str) -> bool;
    fn fact(&self, fact: SecurityFact) -> bool;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Evaluation {
    Compliant,
    NonCompliant,
    NotApplicable,
    Unknown,
}

fn matches(expect: Expect, value: &RegistryData) -> Option<bool> {
    match expect {
        Expect::DwordEq(x) => value.as_dword().map(|v| v == x),
        Expect::DwordNe(x) => value.as_dword().map(|v| v != x),
        Expect::DwordAtLeast(x) => value.as_dword().map(|v| v >= x),
        Expect::DwordAtMost(x) => value.as_dword().map(|v| v <= x),
        Expect::DwordAllBits(mask) => value.as_dword().map(|v| v & mask == mask),
        Expect::StrNe(s) => value.as_str().map(|v| !v.trim().eq_ignore_ascii_case(s)),
        Expect::StrEq(s) => value.as_str().map(|v| v.trim().eq_ignore_ascii_case(s)),
        Expect::Absent => Some(false),
    }
}

fn cond_holds(cond: &Cond, reader: &dyn SettingReader) -> bool {
    match cond {
        Cond::Value(setting, expect) => match reader.read(setting) {
            Ok(Some(value)) => matches(*expect, &value).unwrap_or(false),
            Ok(None) => *expect == Expect::Absent,
            Err(()) => false,
        },
        Cond::KeyExists(root, subkey) => reader.key_exists(*root, subkey),
        Cond::Fact(fact) => reader.fact(*fact),
    }
}

/// Pure evaluation of one check.
pub fn evaluate(check: &SecurityCheck, reader: &dyn SettingReader) -> Evaluation {
    if !check.applies.iter().all(|c| cond_holds(c, reader)) {
        return Evaluation::NotApplicable;
    }
    if check.also_compliant.iter().any(|c| cond_holds(c, reader)) {
        return Evaluation::Compliant;
    }
    let value = match reader.read(&check.setting) {
        Ok(v) => v,
        Err(()) => return Evaluation::Unknown,
    };
    let verdict = match value {
        Some(value) => matches(check.expect, &value),
        None if check.expect == Expect::Absent => Some(true),
        None => match check.absent {
            WhenAbsent::Dword(v) => matches(check.expect, &RegistryData::Dword(v)),
            WhenAbsent::Str(s) => matches(check.expect, &RegistryData::String(s.into())),
            WhenAbsent::Compliant => Some(true),
            WhenAbsent::NonCompliant => Some(false),
            WhenAbsent::Unknown => None,
        },
    };
    match verdict {
        Some(true) => Evaluation::Compliant,
        Some(false) => Evaluation::NonCompliant,
        // Present but of an unexpected type → do not guess.
        None => Evaluation::Unknown,
    }
}

const LM: RegistryRoot = RegistryRoot::LocalMachine;
const CU: RegistryRoot = RegistryRoot::CurrentUser;

const fn s(root: RegistryRoot, subkey: &'static str, name: &'static str) -> SettingRef {
    SettingRef { root, subkey, name }
}

const POLICIES_SYSTEM: &str = "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Policies\\System";
const POLICIES_EXPLORER: &str = "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Policies\\Explorer";
const LSA: &str = "SYSTEM\\CurrentControlSet\\Control\\Lsa";
const TERMINAL_SERVER: &str = "SYSTEM\\CurrentControlSet\\Control\\Terminal Server";
const DEFENDER: &str = "SOFTWARE\\Microsoft\\Windows Defender";
const DEFENDER_POLICY: &str = "SOFTWARE\\Policies\\Microsoft\\Windows Defender";
const CFA: &str = "SOFTWARE\\Microsoft\\Windows Defender\\Windows Defender Exploit Guard\\Controlled Folder Access";
const CFA_POLICY: &str =
    "SOFTWARE\\Policies\\Microsoft\\Windows Defender\\Windows Defender Exploit Guard\\Controlled Folder Access";
const EXPLORER_ADVANCED: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\Advanced";
const WINLOGON: &str = "SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\Winlogon";

use CareRisk::{Info, Review, Safe};

const fn check(
    id: &'static str,
    setting: SettingRef,
    expect: Expect,
    absent: WhenAbsent,
    fix: Option<SecurityFix>,
    risk: CareRisk,
) -> SecurityCheck {
    SecurityCheck {
        id,
        needs_admin: matches!(setting.root, RegistryRoot::LocalMachine),
        setting,
        expect,
        absent,
        fix,
        risk,
        applies: &[],
        also_compliant: &[],
        action: None,
    }
}

const fn write(v: u32) -> Option<SecurityFix> {
    Some(SecurityFix::Write(Desired::Dword(v)))
}

const DELETE: Option<SecurityFix> = Some(SecurityFix::Delete);

/// The checklist, in display order.
pub const SECURITY_CHECKS: &[SecurityCheck] = &[
    // --- User Account Control -------------------------------------------------------------
    check("uacEnabled", s(LM, POLICIES_SYSTEM, "EnableLUA"), Expect::DwordEq(1), WhenAbsent::Dword(1), write(1), Safe),
    check(
        "uacSecureDesktop",
        s(LM, POLICIES_SYSTEM, "PromptOnSecureDesktop"),
        Expect::DwordEq(1),
        WhenAbsent::Dword(1),
        write(1),
        Safe,
    ),
    check(
        "uacAdminPrompt",
        s(LM, POLICIES_SYSTEM, "ConsentPromptBehaviorAdmin"),
        Expect::DwordNe(0),
        WhenAbsent::Dword(5),
        write(5),
        Safe,
    ),
    check(
        "remoteUacFiltering",
        s(LM, POLICIES_SYSTEM, "LocalAccountTokenFilterPolicy"),
        Expect::DwordNe(1),
        WhenAbsent::Compliant,
        DELETE,
        Safe,
    ),
    // --- SmartScreen ----------------------------------------------------------------------
    check(
        "smartScreenApps",
        s(LM, "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Explorer", "SmartScreenEnabled"),
        Expect::StrNe("Off"),
        WhenAbsent::Compliant,
        Some(SecurityFix::Write(Desired::Str("Warn"))),
        Safe,
    ),
    check(
        "smartScreenPolicy",
        s(LM, "SOFTWARE\\Policies\\Microsoft\\Windows\\System", "EnableSmartScreen"),
        Expect::DwordNe(0),
        WhenAbsent::Compliant,
        DELETE,
        Review,
    ),
    check(
        "smartScreenStoreApps",
        s(CU, "Software\\Microsoft\\Windows\\CurrentVersion\\AppHost", "EnableWebContentEvaluation"),
        Expect::DwordNe(0),
        WhenAbsent::Compliant,
        write(1),
        Safe,
    ),
    // --- AutoRun / AutoPlay ---------------------------------------------------------------
    check(
        "autorunAllDrives",
        s(LM, POLICIES_EXPLORER, "NoDriveTypeAutoRun"),
        Expect::DwordAllBits(0xFF),
        WhenAbsent::Dword(0x91),
        write(0xFF),
        Safe,
    ),
    check("autorunInf", s(LM, POLICIES_EXPLORER, "NoAutorun"), Expect::DwordEq(1), WhenAbsent::NonCompliant, write(1), Safe),
    check(
        "autoplay",
        s(CU, "Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\AutoplayHandlers", "DisableAutoplay"),
        Expect::DwordEq(1),
        WhenAbsent::Dword(0),
        write(1),
        Review,
    ),
    // --- Network / authentication ---------------------------------------------------------
    SecurityCheck {
        // Only meaningful while the deprecated SMB 1.0 feature is installed.
        applies: &[Cond::KeyExists(LM, "SYSTEM\\CurrentControlSet\\Services\\mrxsmb10")],
        ..check(
            "smb1Server",
            s(LM, "SYSTEM\\CurrentControlSet\\Services\\LanmanServer\\Parameters", "SMB1"),
            Expect::DwordEq(0),
            WhenAbsent::NonCompliant,
            write(0),
            Safe,
        )
    },
    check("restrictAnonymousSam", s(LM, LSA, "RestrictAnonymousSAM"), Expect::DwordEq(1), WhenAbsent::Dword(1), write(1), Safe),
    check("restrictAnonymous", s(LM, LSA, "RestrictAnonymous"), Expect::DwordAtLeast(1), WhenAbsent::Dword(0), write(1), Review),
    check(
        "everyoneIncludesAnonymous",
        s(LM, LSA, "EveryoneIncludesAnonymous"),
        Expect::DwordEq(0),
        WhenAbsent::Dword(0),
        write(0),
        Safe,
    ),
    check("noLmHash", s(LM, LSA, "NoLMHash"), Expect::DwordEq(1), WhenAbsent::Dword(1), write(1), Safe),
    check("lmCompatibility", s(LM, LSA, "LmCompatibilityLevel"), Expect::DwordAtLeast(3), WhenAbsent::Dword(3), write(3), Safe),
    check(
        "limitBlankPasswords",
        s(LM, LSA, "LimitBlankPasswordUse"),
        Expect::DwordEq(1),
        WhenAbsent::Dword(1),
        write(1),
        Safe,
    ),
    check(
        "wdigestCredentials",
        s(LM, "SYSTEM\\CurrentControlSet\\Control\\SecurityProviders\\WDigest", "UseLogonCredential"),
        Expect::DwordEq(0),
        WhenAbsent::Dword(0),
        write(0),
        Safe,
    ),
    SecurityCheck {
        also_compliant: &[Cond::Value(s(LM, LSA, "RunAsPPLBoot"), Expect::DwordAtLeast(1))],
        ..check("lsaProtection", s(LM, LSA, "RunAsPPL"), Expect::DwordAtLeast(1), WhenAbsent::Dword(0), write(1), Review)
    },
    check(
        "llmnr",
        s(LM, "SOFTWARE\\Policies\\Microsoft\\Windows NT\\DNSClient", "EnableMulticast"),
        Expect::DwordEq(0),
        WhenAbsent::NonCompliant,
        write(0),
        Review,
    ),
    // --- Exploit mitigation / scripting ---------------------------------------------------
    check(
        "sehop",
        s(LM, "SYSTEM\\CurrentControlSet\\Control\\Session Manager\\kernel", "DisableExceptionChainValidation"),
        Expect::DwordEq(0),
        WhenAbsent::Dword(0),
        write(0),
        Safe,
    ),
    check(
        "windowsScriptHost",
        s(CU, "Software\\Microsoft\\Windows Script Host\\Settings", "Enabled"),
        Expect::DwordEq(0),
        WhenAbsent::Dword(1),
        write(0),
        Review,
    ),
    check(
        "alwaysInstallElevatedMachine",
        s(LM, "SOFTWARE\\Policies\\Microsoft\\Windows\\Installer", "AlwaysInstallElevated"),
        Expect::DwordNe(1),
        WhenAbsent::Compliant,
        DELETE,
        Safe,
    ),
    SecurityCheck {
        // HKCU\Software\Policies is writable by administrators only.
        needs_admin: true,
        ..check(
            "alwaysInstallElevatedUser",
            s(CU, "Software\\Policies\\Microsoft\\Windows\\Installer", "AlwaysInstallElevated"),
            Expect::DwordNe(1),
            WhenAbsent::Compliant,
            DELETE,
            Safe,
        )
    },
    SecurityCheck {
        also_compliant: &[Cond::Value(
            s(LM, "SOFTWARE\\Policies\\Microsoft\\Windows NT\\Printers\\PointAndPrint", "RestrictDriverInstallationToAdministrators"),
            Expect::DwordEq(1),
        )],
        ..check(
            "pointAndPrint",
            s(LM, "SOFTWARE\\Policies\\Microsoft\\Windows NT\\Printers\\PointAndPrint", "NoWarningNoElevationOnInstall"),
            Expect::DwordNe(1),
            WhenAbsent::Compliant,
            DELETE,
            Safe,
        )
    },
    // --- Remote access --------------------------------------------------------------------
    check("remoteDesktop", s(LM, TERMINAL_SERVER, "fDenyTSConnections"), Expect::DwordEq(1), WhenAbsent::Dword(1), write(1), Review),
    SecurityCheck {
        applies: &[Cond::Value(s(LM, TERMINAL_SERVER, "fDenyTSConnections"), Expect::DwordEq(0))],
        ..check(
            "remoteDesktopNla",
            s(LM, "SYSTEM\\CurrentControlSet\\Control\\Terminal Server\\WinStations\\RDP-Tcp", "UserAuthentication"),
            Expect::DwordEq(1),
            WhenAbsent::Dword(1),
            write(1),
            Safe,
        )
    },
    check(
        "remoteAssistance",
        s(LM, "SYSTEM\\CurrentControlSet\\Control\\Remote Assistance", "fAllowToGetHelp"),
        Expect::DwordEq(0),
        WhenAbsent::Dword(1),
        write(0),
        Review,
    ),
    // --- Informational --------------------------------------------------------------------
    SecurityCheck {
        // The stored password itself is never read into results or undo data (no fix).
        applies: &[Cond::Value(s(LM, WINLOGON, "AutoAdminLogon"), Expect::StrEq("1"))],
        ..check("autoLogonPassword", s(LM, WINLOGON, "DefaultPassword"), Expect::Absent, WhenAbsent::Compliant, None, Info)
    },
    SecurityCheck {
        applies: &[Cond::KeyExists(LM, "SYSTEM\\CurrentControlSet\\Control\\SecureBoot\\State")],
        ..check(
            "secureBoot",
            s(LM, "SYSTEM\\CurrentControlSet\\Control\\SecureBoot\\State", "UEFISecureBootEnabled"),
            Expect::DwordEq(1),
            WhenAbsent::Unknown,
            None,
            Info,
        )
    },
    // --- Microsoft Defender ---------------------------------------------------------------
    SecurityCheck {
        applies: &[Cond::Fact(SecurityFact::DefenderActive)],
        also_compliant: &[Cond::Value(s(LM, DEFENDER, "PUAProtection"), Expect::DwordEq(1))],
        ..check("defenderPua", s(LM, DEFENDER_POLICY, "PUAProtection"), Expect::DwordEq(1), WhenAbsent::NonCompliant, write(1), Review)
    },
    SecurityCheck {
        applies: &[Cond::Fact(SecurityFact::DefenderActive)],
        also_compliant: &[Cond::Value(s(LM, CFA_POLICY, "EnableControlledFolderAccess"), Expect::DwordEq(1))],
        action: Some("openWindowsSecurity"),
        ..check(
            "controlledFolderAccess",
            s(LM, CFA, "EnableControlledFolderAccess"),
            Expect::DwordEq(1),
            WhenAbsent::NonCompliant,
            None,
            Info,
        )
    },
    // --- Privacy-related settings (per user unless noted) ---------------------------------
    check(
        "advertisingId",
        s(CU, "Software\\Microsoft\\Windows\\CurrentVersion\\AdvertisingInfo", "Enabled"),
        Expect::DwordEq(0),
        WhenAbsent::Dword(1),
        write(0),
        Safe,
    ),
    check(
        "tailoredExperiences",
        s(CU, "Software\\Microsoft\\Windows\\CurrentVersion\\Privacy", "TailoredExperiencesWithDiagnosticDataEnabled"),
        Expect::DwordEq(0),
        WhenAbsent::Dword(1),
        write(0),
        Safe,
    ),
    check(
        "suggestedContent",
        s(CU, "Software\\Microsoft\\Windows\\CurrentVersion\\ContentDeliveryManager", "SubscribedContent-338393Enabled"),
        Expect::DwordEq(0),
        WhenAbsent::Dword(1),
        write(0),
        Safe,
    ),
    check(
        "languageListAccess",
        s(CU, "Control Panel\\International\\User Profile", "HttpAcceptLanguageOptOut"),
        Expect::DwordEq(1),
        WhenAbsent::Dword(0),
        write(1),
        Safe,
    ),
    check(
        "inkingTypingPersonalization",
        s(CU, "Software\\Microsoft\\Input\\TIPC", "Enabled"),
        Expect::DwordEq(0),
        WhenAbsent::Dword(1),
        write(0),
        Safe,
    ),
    check("appLaunchTracking", s(CU, EXPLORER_ADVANCED, "Start_TrackProgs"), Expect::DwordEq(0), WhenAbsent::Dword(1), write(0), Review),
    SecurityCheck {
        also_compliant: &[Cond::Value(
            s(LM, "SOFTWARE\\Policies\\Microsoft\\Windows\\DataCollection", "AllowTelemetry"),
            Expect::DwordAtMost(1),
        )],
        ..check(
            "telemetryLevel",
            s(LM, "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Policies\\DataCollection", "AllowTelemetry"),
            Expect::DwordAtMost(1),
            WhenAbsent::Dword(3),
            write(1),
            Review,
        )
    },
    check("showFileExtensions", s(CU, EXPLORER_ADVANCED, "HideFileExt"), Expect::DwordEq(0), WhenAbsent::Dword(1), write(0), Safe),
];

pub fn find_check(id: &str) -> Option<&'static SecurityCheck> {
    SECURITY_CHECKS.iter().find(|c| c.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[derive(Default)]
    struct FakeReader {
        values: HashMap<(&'static str, &'static str), RegistryData>,
        denied: Vec<&'static str>,
        keys: Vec<&'static str>,
        defender: bool,
    }

    impl FakeReader {
        fn with(mut self, subkey: &'static str, name: &'static str, value: RegistryData) -> Self {
            self.values.insert((subkey, name), value);
            self
        }
    }

    impl SettingReader for FakeReader {
        fn read(&self, setting: &SettingRef) -> Result<Option<RegistryData>, ()> {
            if self.denied.contains(&setting.name) {
                return Err(());
            }
            Ok(self.values.get(&(setting.subkey, setting.name)).cloned())
        }
        fn key_exists(&self, _root: RegistryRoot, subkey: &str) -> bool {
            self.keys.iter().any(|k| *k == subkey)
        }
        fn fact(&self, fact: SecurityFact) -> bool {
            match fact {
                SecurityFact::DefenderActive => self.defender,
            }
        }
    }

    fn eval(id: &str, reader: &FakeReader) -> Evaluation {
        evaluate(find_check(id).unwrap(), reader)
    }

    #[test]
    fn table_ids_are_unique_and_safe_items_are_fixable_and_reversible() {
        let mut ids: Vec<_> = SECURITY_CHECKS.iter().map(|c| c.id).collect();
        let total = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), total, "duplicate check id");
        assert!(total >= 25);
        for c in SECURITY_CHECKS {
            if c.fix.is_none() {
                assert_eq!(c.risk, CareRisk::Info, "{} has no fix but is not Info", c.id);
            } else {
                assert_ne!(c.risk, CareRisk::Info, "{} is fixable but Info", c.id);
            }
            if c.setting.root == RegistryRoot::LocalMachine {
                assert!(c.needs_admin, "{} writes HKLM without needs_admin", c.id);
            }
        }
    }

    #[test]
    fn every_fix_makes_its_check_compliant() {
        for c in SECURITY_CHECKS.iter().filter(|c| c.fix.is_some()) {
            let after = match c.fix.unwrap() {
                SecurityFix::Write(d) => FakeReader::default().with(c.setting.subkey, c.setting.name, d.to_data()),
                SecurityFix::Delete => FakeReader::default(),
            };
            let mut after = after;
            after.defender = true;
            after.keys = vec!["SYSTEM\\CurrentControlSet\\Services\\mrxsmb10"];
            after.values.insert((TERMINAL_SERVER, "fDenyTSConnections"), RegistryData::Dword(0));
            if c.id == "remoteDesktop" {
                after.values.insert((TERMINAL_SERVER, "fDenyTSConnections"), RegistryData::Dword(1));
            }
            assert_eq!(evaluate(c, &after), Evaluation::Compliant, "{} fix does not satisfy its check", c.id);
        }
    }

    #[test]
    fn every_check_has_english_text() {
        let en: serde_json::Value =
            serde_json::from_str(include_str!("../../../../../src/locales/en.json")).expect("en.json");
        for c in SECURITY_CHECKS {
            for field in ["title", "detail"] {
                assert!(
                    en["care"]["security"]["checks"][c.id][field].is_string(),
                    "missing care.security.checks.{}.{field}",
                    c.id
                );
            }
        }
    }

    #[test]
    fn absent_values_use_windows_defaults() {
        let empty = FakeReader::default();
        assert_eq!(eval("uacEnabled", &empty), Evaluation::Compliant);
        assert_eq!(eval("showFileExtensions", &empty), Evaluation::NonCompliant);
        assert_eq!(eval("autorunAllDrives", &empty), Evaluation::NonCompliant);
        assert_eq!(eval("lmCompatibility", &empty), Evaluation::Compliant);
        assert_eq!(eval("smartScreenApps", &empty), Evaluation::Compliant);
    }

    #[test]
    fn present_values_are_compared() {
        let r = FakeReader::default()
            .with(POLICIES_SYSTEM, "ConsentPromptBehaviorAdmin", RegistryData::Dword(0))
            .with("SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Explorer", "SmartScreenEnabled", RegistryData::String("off".into()))
            .with(LSA, "LmCompatibilityLevel", RegistryData::Dword(1))
            .with(POLICIES_EXPLORER, "NoDriveTypeAutoRun", RegistryData::Dword(0xFF));
        assert_eq!(eval("uacAdminPrompt", &r), Evaluation::NonCompliant);
        assert_eq!(eval("smartScreenApps", &r), Evaluation::NonCompliant);
        assert_eq!(eval("lmCompatibility", &r), Evaluation::NonCompliant);
        assert_eq!(eval("autorunAllDrives", &r), Evaluation::Compliant);
    }

    #[test]
    fn wrong_type_or_denied_read_is_unknown() {
        let r = FakeReader::default().with(POLICIES_SYSTEM, "EnableLUA", RegistryData::Other(3));
        assert_eq!(eval("uacEnabled", &r), Evaluation::Unknown);
        let denied = FakeReader { denied: vec!["EnableLUA"], ..Default::default() };
        assert_eq!(eval("uacEnabled", &denied), Evaluation::Unknown);
    }

    #[test]
    fn conditions_gate_and_override() {
        let none = FakeReader::default();
        assert_eq!(eval("smb1Server", &none), Evaluation::NotApplicable);
        assert_eq!(eval("defenderPua", &none), Evaluation::NotApplicable);
        assert_eq!(eval("remoteDesktopNla", &none), Evaluation::NotApplicable);
        assert_eq!(eval("autoLogonPassword", &none), Evaluation::NotApplicable);

        let smb = FakeReader { keys: vec!["SYSTEM\\CurrentControlSet\\Services\\mrxsmb10"], ..Default::default() };
        assert_eq!(eval("smb1Server", &smb), Evaluation::NonCompliant);

        let pua_ui = FakeReader { defender: true, ..Default::default() }.with(DEFENDER, "PUAProtection", RegistryData::Dword(1));
        assert_eq!(eval("defenderPua", &pua_ui), Evaluation::Compliant);
        let pua_off = FakeReader { defender: true, ..Default::default() };
        assert_eq!(eval("defenderPua", &pua_off), Evaluation::NonCompliant);

        let autologon = FakeReader::default()
            .with(WINLOGON, "AutoAdminLogon", RegistryData::String("1".into()))
            .with(WINLOGON, "DefaultPassword", RegistryData::String("x".into()));
        assert_eq!(eval("autoLogonPassword", &autologon), Evaluation::NonCompliant);

        let ppl_boot = FakeReader::default().with(LSA, "RunAsPPLBoot", RegistryData::Dword(2));
        assert_eq!(eval("lsaProtection", &ppl_boot), Evaluation::Compliant);
    }
}

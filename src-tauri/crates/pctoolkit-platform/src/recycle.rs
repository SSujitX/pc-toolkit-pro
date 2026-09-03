use crate::{PlatformError, PlatformResult};

/// Current-user Recycle Bin size as Explorer reports it (`SHQueryRecycleBin`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RecycleBinInfo {
    pub bytes: u64,
    pub item_count: u64,
}

/// Result of emptying the Recycle Bin (before minus after Shell query).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RecycleBinEmptyResult {
    pub released_bytes: u64,
    pub released_items: u64,
}

/// Convert Shell `i64` fields. Negative values are treated as a failed query.
pub(crate) fn snapshot_from_shell(bytes: i64, item_count: i64) -> Option<RecycleBinInfo> {
    if bytes < 0 || item_count < 0 {
        return None;
    }
    Some(RecycleBinInfo {
        bytes: bytes as u64,
        item_count: item_count as u64,
    })
}

pub fn query_recycle_bin() -> PlatformResult<RecycleBinInfo> {
    #[cfg(windows)]
    {
        query_recycle_bin_windows()
    }
    #[cfg(not(windows))]
    {
        Err(PlatformError::Unsupported)
    }
}

pub fn empty_recycle_bin() -> PlatformResult<RecycleBinEmptyResult> {
    #[cfg(windows)]
    {
        empty_recycle_bin_windows()
    }
    #[cfg(not(windows))]
    {
        Err(PlatformError::Unsupported)
    }
}

#[cfg(windows)]
fn query_recycle_bin_windows() -> PlatformResult<RecycleBinInfo> {
    use windows::core::PCWSTR;
    use windows::Win32::UI::Shell::{SHQueryRecycleBinW, SHQUERYRBINFO};

    let mut info = SHQUERYRBINFO {
        cbSize: std::mem::size_of::<SHQUERYRBINFO>() as u32,
        i64Size: 0,
        i64NumItems: 0,
    };
    // NULL root = all drives, current user — same contract Explorer uses.
    unsafe { SHQueryRecycleBinW(PCWSTR::null(), &mut info) }.map_err(|e| {
        PlatformError::OperationFailed(format!("recycleBinQueryFailed:{e:?}"))
    })?;
    snapshot_from_shell(info.i64Size, info.i64NumItems).ok_or_else(|| {
        PlatformError::OperationFailed("recycleBinQueryNegative".into())
    })
}

#[cfg(windows)]
fn empty_recycle_bin_windows() -> PlatformResult<RecycleBinEmptyResult> {
    use windows::core::PCWSTR;
    use windows::Win32::UI::Shell::SHEmptyRecycleBinW;

    let before = query_recycle_bin_windows()?;
    if before.bytes == 0 && before.item_count == 0 {
        return Ok(RecycleBinEmptyResult::default());
    }

    // SHERB_NOCONFIRMATION | SHERB_NOPROGRESSUI | SHERB_NOSOUND
    const EMPTY_FLAGS: u32 = 0x0000_0001 | 0x0000_0002 | 0x0000_0004;
    unsafe { SHEmptyRecycleBinW(None, PCWSTR::null(), EMPTY_FLAGS) }.map_err(|e| {
        PlatformError::OperationFailed(format!("recycleBinEmptyFailed:{e:?}"))
    })?;

    let after = query_recycle_bin_windows().unwrap_or_default();
    Ok(RecycleBinEmptyResult {
        released_bytes: before.bytes.saturating_sub(after.bytes),
        released_items: before.item_count.saturating_sub(after.item_count),
    })
}

#[cfg(test)]
mod tests {
    use super::snapshot_from_shell;

    #[test]
    fn snapshot_from_shell_rejects_negative_counts() {
        assert_eq!(snapshot_from_shell(-1, 3), None);
        assert_eq!(snapshot_from_shell(10, -1), None);
        assert_eq!(
            snapshot_from_shell(10, 3).map(|s| (s.bytes, s.item_count)),
            Some((10, 3))
        );
    }
}

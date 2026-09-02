use serde::Serialize;
use std::process::Command;

use crate::{PlatformError, PlatformResult};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuSample {
    pub name: String,
    pub utilization: f32,
    pub memory_used: u64,
    pub memory_total: u64,
    pub temperature: f32,
}

pub fn sample_nvidia() -> PlatformResult<GpuSample> {
    let mut cmd = Command::new("nvidia-smi");
    cmd.args([
        "--query-gpu=name,utilization.gpu,memory.used,memory.total,temperature.gpu",
        "--format=csv,noheader,nounits",
    ]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    let output = cmd
        .output()
        .map_err(|e| PlatformError::OperationFailed(e.to_string()))?;

    if !output.status.success() {
        return Err(PlatformError::OperationFailed("nvidia-smi failed".into()));
    }

    let line = String::from_utf8_lossy(&output.stdout);
    let line = line.lines().next().unwrap_or("").trim();
    let parts: Vec<&str> = line.split(',').map(|s| s.trim()).collect();
    if parts.len() < 5 {
        return Err(PlatformError::OperationFailed(
            "unexpected nvidia-smi output".into(),
        ));
    }

    Ok(GpuSample {
        name: parts[0].to_string(),
        utilization: parts[1].parse().unwrap_or(0.0),
        memory_used: parts[2].parse::<u64>().unwrap_or(0) * 1024 * 1024,
        memory_total: parts[3].parse::<u64>().unwrap_or(0) * 1024 * 1024,
        temperature: parts[4].parse().unwrap_or(0.0),
    })
}

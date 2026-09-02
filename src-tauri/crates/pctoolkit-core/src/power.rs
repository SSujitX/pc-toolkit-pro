use pctoolkit_platform::{
    cancel_scheduled_shutdown as platform_cancel, execute_power_action, schedule_shutdown as platform_schedule,
};
use serde::{Deserialize, Serialize};

use crate::history::record_history;
use crate::shared::CoreResult;

pub use pctoolkit_platform::PowerAction;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScheduleRequest {
    pub seconds: u64,
}

pub fn execute_power(action: PowerAction) -> CoreResult<()> {
    execute_power_action(action)?;
    record_history("power", format!("{action:?}"), true, None);
    Ok(())
}

pub fn schedule_shutdown(request: ScheduleRequest) -> CoreResult<()> {
    if request.seconds == 0 || request.seconds > 86400 {
        return Err(crate::CoreError::InvalidInput(
            "seconds must be 1..=86400".into(),
        ));
    }
    platform_schedule(request.seconds)?;
    record_history(
        "power",
        format!("schedule_shutdown_{}", request.seconds),
        true,
        None,
    );
    Ok(())
}

pub fn cancel_scheduled_shutdown() -> CoreResult<()> {
    platform_cancel()?;
    record_history("power", "cancel_scheduled_shutdown".into(), true, None);
    Ok(())
}

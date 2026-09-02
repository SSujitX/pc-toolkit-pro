use pctoolkit_platform::{
    cancel_scheduled_shutdown as platform_cancel, execute_power_action,
    schedule_shutdown as platform_schedule,
};
use serde::{Deserialize, Serialize};

use crate::history::{append_history, history_now_ms, HistoryOutcome, HistoryWrite};
use crate::shared::CoreResult;

pub use pctoolkit_platform::PowerAction;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScheduleRequest {
    pub seconds: u64,
}

pub fn execute_power(action: PowerAction) -> CoreResult<()> {
    let started = history_now_ms();
    execute_power_action(action)?;
    append_history(HistoryWrite {
        category: "power".into(),
        title_key: "history.titles.power".into(),
        summary: format!("{action:?}"),
        started_at_ms: started,
        finished_at_ms: history_now_ms(),
        outcome: HistoryOutcome::Completed,
        planned_bytes: None,
        result_bytes: None,
        selected_item_count: 1,
        affected_item_count: 1,
        failed_item_count: 0,
        detail_lines: vec![format!("Power action: {action:?}")],
        action: format!("{action:?}"),
        detail: None,
    });
    Ok(())
}

pub fn schedule_shutdown(request: ScheduleRequest) -> CoreResult<()> {
    if request.seconds == 0 || request.seconds > 86400 {
        return Err(crate::CoreError::InvalidInput(
            "seconds must be 1..=86400".into(),
        ));
    }
    let started = history_now_ms();
    platform_schedule(request.seconds)?;
    append_history(HistoryWrite {
        category: "power".into(),
        title_key: "history.titles.scheduleShutdown".into(),
        summary: format!("Delay {} seconds", request.seconds),
        started_at_ms: started,
        finished_at_ms: history_now_ms(),
        outcome: HistoryOutcome::Completed,
        planned_bytes: None,
        result_bytes: None,
        selected_item_count: 1,
        affected_item_count: 1,
        failed_item_count: 0,
        detail_lines: vec![format!(
            "Scheduled shutdown in {} seconds",
            request.seconds
        )],
        action: format!("schedule_shutdown_{}", request.seconds),
        detail: Some(format!("seconds={}", request.seconds)),
    });
    Ok(())
}

pub fn cancel_scheduled_shutdown() -> CoreResult<()> {
    let started = history_now_ms();
    platform_cancel()?;
    append_history(HistoryWrite {
        category: "power".into(),
        title_key: "history.titles.cancelSchedule".into(),
        summary: "Scheduled shutdown cancelled".into(),
        started_at_ms: started,
        finished_at_ms: history_now_ms(),
        outcome: HistoryOutcome::Completed,
        planned_bytes: None,
        result_bytes: None,
        selected_item_count: 1,
        affected_item_count: 1,
        failed_item_count: 0,
        detail_lines: vec!["Cancelled the pending scheduled shutdown.".into()],
        action: "cancel_scheduled_shutdown".into(),
        detail: None,
    });
    Ok(())
}

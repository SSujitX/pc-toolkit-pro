use crate::commands::error::{run_blocking, CommandResult};

#[tauri::command]
pub async fn get_system_information() -> CommandResult<pctoolkit_core::system_info::SystemInformationDto>
{
    run_blocking("get_system_information", pctoolkit_core::get_system_information).await
}

/** Mirrors Rust `pctoolkit_core::app_settings`. */
export interface AppStartupSettings {
  startWithWindows: boolean;
  startMinimized: boolean;
  runAsAdmin: boolean;
}

export type StartupRegistrationState =
  | 'off'
  | 'runValue'
  | 'elevatedTask'
  | 'needsAdminToCreateTask'
  | 'needsAdminToRemoveTask'
  | 'failed';

export interface AppStartupStatus {
  settings: AppStartupSettings;
  registration: StartupRegistrationState;
  elevated: boolean;
}

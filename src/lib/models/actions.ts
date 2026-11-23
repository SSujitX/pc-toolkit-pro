export type QuickActionId =
  | 'taskManager'
  | 'deviceManager'
  | 'controlPanel'
  | 'diskManagement'
  | 'commandPromptAdmin'
  | 'powerShellAdmin'
  | 'systemInfo'
  | 'registryEditor'
  | 'settings'
  | 'services'
  | 'fileExplorer'
  | 'networkConnections'
  | 'snippingTool'
  | 'notepad'
  | 'volumeMixer'
  | 'eventViewer'
  | 'resourceMonitor'
  | 'reliabilityMonitor'
  | 'computerManagement'
  | 'taskScheduler'
  | 'systemProperties'
  | 'environmentVariables'
  | 'directXDiagnostic'
  | 'diskCleanup'
  | 'defragmentOptimize'
  | 'systemRestore'
  | 'memoryDiagnostic'
  | 'windowsUpdate'
  | 'windowsSecurity'
  | 'startupApps'
  | 'installedApps'
  | 'storageSettings'
  | 'powerOptions'
  | 'soundSettings'
  | 'displaySettings'
  | 'firewall';

export const QUICK_ACTIONS: { id: QuickActionId; labelKey: string }[] = [
  { id: 'taskManager', labelKey: 'actions.taskManager' },
  { id: 'deviceManager', labelKey: 'actions.deviceManager' },
  { id: 'controlPanel', labelKey: 'actions.controlPanel' },
  { id: 'diskManagement', labelKey: 'actions.diskManagement' },
  { id: 'commandPromptAdmin', labelKey: 'actions.commandPromptAdmin' },
  { id: 'powerShellAdmin', labelKey: 'actions.powerShellAdmin' },
  { id: 'systemInfo', labelKey: 'actions.systemInfo' },
  { id: 'registryEditor', labelKey: 'actions.registryEditor' },
  { id: 'settings', labelKey: 'actions.settings' },
  { id: 'services', labelKey: 'actions.services' },
  { id: 'fileExplorer', labelKey: 'actions.fileExplorer' },
  { id: 'networkConnections', labelKey: 'actions.networkConnections' },
  { id: 'snippingTool', labelKey: 'actions.snippingTool' },
  { id: 'notepad', labelKey: 'actions.notepad' },
  { id: 'volumeMixer', labelKey: 'actions.volumeMixer' },
];

export type QuickActionGroup = 'windowsTools' | 'systemTools' | 'maintenance' | 'settings';

/** Quick actions grouped for the System page (each opens Windows' own tool). */
export const QUICK_ACTION_GROUPS: { id: QuickActionGroup; actions: QuickActionId[] }[] = [
  { id: 'windowsTools', actions: QUICK_ACTIONS.map((a) => a.id) },
  { id: 'systemTools', actions: ['eventViewer', 'resourceMonitor', 'reliabilityMonitor', 'computerManagement', 'taskScheduler', 'systemProperties', 'environmentVariables', 'directXDiagnostic'] },
  { id: 'maintenance', actions: ['diskCleanup', 'defragmentOptimize', 'systemRestore', 'memoryDiagnostic', 'windowsUpdate', 'windowsSecurity'] },
  { id: 'settings', actions: ['startupApps', 'installedApps', 'storageSettings', 'powerOptions', 'soundSettings', 'displaySettings', 'firewall'] },
];


export type PowerAction =
  | 'shutdown'
  | 'restart'
  | 'sleep'
  | 'hibernate'
  | 'lock'
  | 'signOut';

export type CleanerCategory = 'tempFiles' | 'recycleBin' | 'diskCleanup' | 'freeMemory';

/** One-click Cleaner presets (Python-era actions, professional UX). */
export type CleanerPreset =
  | 'tempPrefetchRecycle'
  | 'diskCleanup'
  | 'recycleOnly'
  | 'freeMemory';

export const CLEANER_PRESET_CATEGORIES: Record<CleanerPreset, CleanerCategory[]> = {
  tempPrefetchRecycle: ['tempFiles', 'recycleBin'],
  diskCleanup: ['diskCleanup'],
  recycleOnly: ['recycleBin'],
  freeMemory: ['freeMemory'],
};

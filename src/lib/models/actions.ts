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
  | 'volumeMixer';

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

export type PowerAction =
  | 'shutdown'
  | 'restart'
  | 'sleep'
  | 'hibernate'
  | 'lock'
  | 'signOut';

export type CleanerCategory = 'tempFiles' | 'recycleBin' | 'diskCleanup' | 'freeMemory';

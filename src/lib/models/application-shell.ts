export const APP_NAME = 'PC Toolkit Pro';
export const APP_VERSION = '3.0.0';

export const PAGE_IDS = {
  monitor: 'monitor',
  cleaner: 'cleaner',
  power: 'power',
  information: 'information',
  settings: 'settings',
} as const;

export type PageId = (typeof PAGE_IDS)[keyof typeof PAGE_IDS];

export const PRIMARY_NAV = [
  { id: PAGE_IDS.monitor, icon: 'gauge', group: 'system' },
  { id: PAGE_IDS.cleaner, icon: 'broom', group: 'system' },
  { id: PAGE_IDS.power, icon: 'power', group: 'system' },
  { id: PAGE_IDS.information, icon: 'info', group: 'system' },
] as const;

export const SECONDARY_NAV = [{ id: PAGE_IDS.settings, icon: 'settings' }] as const;

export function createSidebarLayoutState(width: number) {
  return {
    expanded: width >= 1100,
    userToggled: false,
  };
}

export const APP_NAME = 'PC Toolkit Pro';
export const APP_VERSION = '3.0.0';

export const PAGE_IDS = {
  monitor: 'monitor',
  cleaner: 'cleaner',
  deepCleaner: 'deepCleaner',
  memoryCleaner: 'memoryCleaner',
  power: 'power',
  information: 'information',
  history: 'history',
  settings: 'settings',
} as const;

export type PageId = (typeof PAGE_IDS)[keyof typeof PAGE_IDS];

export type NavIconId =
  | 'gauge'
  | 'broom'
  | 'layers'
  | 'brain'
  | 'power'
  | 'info'
  | 'history'
  | 'settings';

export interface NavItem {
  id: PageId;
  icon: NavIconId;
}

export interface NavGroup {
  id: string;
  titleKey: string;
  items: readonly NavItem[];
}

/** Categorized sidebar (PC Toolkit Pro tool set). */
export const PRIMARY_NAV_GROUPS: readonly NavGroup[] = [
  {
    id: 'cleanup',
    titleKey: 'navigation.groups.cleanup',
    items: [
      { id: PAGE_IDS.cleaner, icon: 'broom' },
      { id: PAGE_IDS.deepCleaner, icon: 'layers' },
      { id: PAGE_IDS.memoryCleaner, icon: 'brain' },
    ],
  },
  {
    id: 'system',
    titleKey: 'navigation.groups.system',
    items: [
      { id: PAGE_IDS.monitor, icon: 'gauge' },
      { id: PAGE_IDS.power, icon: 'power' },
      { id: PAGE_IDS.information, icon: 'info' },
    ],
  },
] as const;

export const SECONDARY_NAV: readonly NavItem[] = [
  { id: PAGE_IDS.history, icon: 'history' },
  { id: PAGE_IDS.settings, icon: 'settings' },
] as const;

/** Flat list kept for preload / busy checks. */
export const PRIMARY_NAV = PRIMARY_NAV_GROUPS.flatMap((group) => group.items);

export function createSidebarLayoutState(width: number) {
  return {
    expanded: width >= 1100,
    userToggled: false,
  };
}

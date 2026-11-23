import { defineStore } from 'pinia';
import { MemoryCleanerService, StartupService } from '@/lib/services/api-services';
import {
  STARTUP_FILTERS,
  matchesStartupFilter,
  matchesStartupQuery,
  sortStartupItems,
  startsWithWindows,
  type BootSummary,
  type StartupActionKind,
  type StartupActionStatus,
  type StartupFilter,
  type StartupItem,
} from '@/lib/models/startup';
import { isCancelledError } from '@/lib/utils/errors';
import { useAppStore } from './app-store';

const ICON_BATCH = 24;

/** Outcome of the last row action, rendered from typed status + locale keys. */
export interface StartupNotice {
  status: StartupActionStatus;
  action: StartupActionKind;
  name: string;
}

function emptyBoot(): BootSummary {
  return { access: 'unavailable', lastBootMs: null, lastBootAtMs: null, bootsRecorded: 0 };
}

export const useStartupStore = defineStore('startup', {
  state: () => ({
    items: [] as StartupItem[],
    isAdmin: false,
    boot: emptyBoot(),
    hiddenMicrosoftCount: 0,
    includeMicrosoft: false,
    loaded: false,
    loading: false,
    filter: 'all' as StartupFilter,
    query: '',
    /** Item id → PNG data URL (`null` = no icon, show initials). */
    icons: {} as Record<string, string | null>,
    /** Ids with an action in flight. */
    pendingIds: [] as string[],
    notice: null as StartupNotice | null,
    /** Admin-only change requested in a non-elevated process. */
    adminPromptItem: null as StartupItem | null,
    restartingElevated: false,
  }),
  getters: {
    visibleItems(state): StartupItem[] {
      return sortStartupItems(
        state.items.filter((i) => matchesStartupFilter(i, state.filter) && matchesStartupQuery(i, state.query))
      );
    },
    filterCounts(state): Record<StartupFilter, number> {
      const counts = {} as Record<StartupFilter, number>;
      for (const filter of STARTUP_FILTERS) {
        counts[filter] = state.items.filter((i) => matchesStartupFilter(i, filter)).length;
      }
      return counts;
    },
    enabledCount(state): number {
      return state.items.filter(startsWithWindows).length;
    },
    highImpactCount(state): number {
      return state.items.filter((i) => startsWithWindows(i) && i.impact === 'high').length;
    },
    busy(state): boolean {
      return state.loading || state.pendingIds.length > 0;
    },
  },
  actions: {
    async ensureLoaded() {
      if (!this.loaded && !this.loading) await this.load();
    },
    async load() {
      if (this.loading) return;
      this.loading = true;
      try {
        const inventory = await StartupService.list(this.includeMicrosoft);
        this.items = inventory.items;
        this.isAdmin = inventory.isAdmin;
        this.boot = inventory.boot;
        this.hiddenMicrosoftCount = inventory.hiddenMicrosoftCount;
        this.loaded = true;
        void this.loadIcons();
      } catch (error) {
        useAppStore().reportError(error);
      } finally {
        this.loading = false;
      }
    },
    async setIncludeMicrosoft(include: boolean) {
      if (this.includeMicrosoft === include) return;
      this.includeMicrosoft = include;
      await this.load();
    },
    async loadIcons() {
      const missing = this.items.map((i) => i.id).filter((id) => !(id in this.icons));
      for (let start = 0; start < missing.length; start += ICON_BATCH) {
        try {
          const icons = await StartupService.getIcons(missing.slice(start, start + ICON_BATCH));
          for (const icon of icons) this.icons[icon.id] = icon.dataUrl;
        } catch {
          // Icons are cosmetic; rows fall back to initials.
          return;
        }
      }
    },
    /** Admin-only rows in a non-elevated process ask to restart elevated instead of trying. */
    async run(item: StartupItem, action: StartupActionKind) {
      if (this.pendingIds.includes(item.id) || this.loading) return;
      if (item.requiresAdmin && !this.isAdmin) {
        this.adminPromptItem = item;
        return;
      }
      this.pendingIds.push(item.id);
      try {
        const result = await StartupService.action(item.id, action);
        if (action === 'delete' && result.status === 'done') {
          this.items = this.items.filter((i) => i.id !== item.id);
        } else if (result.item) {
          const index = this.items.findIndex((i) => i.id === item.id);
          if (index >= 0) this.items[index] = result.item;
        } else if (result.status === 'notFound') {
          this.items = this.items.filter((i) => i.id !== item.id);
        }
        this.notice = { status: result.status, action, name: item.name };
        if (result.status === 'skippedNeedAdmin' && !this.isAdmin) this.adminPromptItem = item;
      } catch (error) {
        useAppStore().reportError(error);
      } finally {
        this.pendingIds = this.pendingIds.filter((id) => id !== item.id);
      }
    },
    toggle(item: StartupItem) {
      return this.run(item, startsWithWindows(item) ? 'disable' : 'enable');
    },
    async reveal(item: StartupItem) {
      try {
        await StartupService.reveal(item.id);
      } catch (error) {
        useAppStore().reportError(error);
      }
    },
    dismissNotice() {
      this.notice = null;
    },
    closeAdminPrompt() {
      this.adminPromptItem = null;
    },
    async restartAsAdministrator() {
      if (this.restartingElevated) return;
      this.restartingElevated = true;
      this.adminPromptItem = null;
      try {
        await MemoryCleanerService.restartAsAdministrator();
      } catch (error) {
        if (!isCancelledError(error)) useAppStore().reportError(error);
      } finally {
        this.restartingElevated = false;
      }
    },
  },
});

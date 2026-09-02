import { defineStore } from 'pinia';
import {
  CleanerService,
  type CleanupProgress,
  type CleanupResult,
} from '@/lib/services/api-services';
import {
  CLEANER_PRESET_CATEGORIES,
  type CleanerPreset,
} from '@/lib/models/actions';
import { PAGE_IDS } from '@/lib/models/application-shell';
import { isCancelledError } from '@/lib/utils/errors';
import { useAppStore } from './app-store';
import { useHistoryStore } from './history-store';

export const useCleanerStore = defineStore('cleaner', {
  state: () => ({
    loading: false,
    activePreset: null as CleanerPreset | null,
    progress: null as CleanupProgress | null,
    result: null as CleanupResult | null,
    log: [] as string[],
  }),
  actions: {
    async runPreset(preset: CleanerPreset) {
      const app = useAppStore();
      const categories = CLEANER_PRESET_CATEGORIES[preset];
      if (!categories.length) return;
      app.setBusy(PAGE_IDS.cleaner, true);
      this.loading = true;
      this.activePreset = preset;
      this.progress = null;
      this.result = null;
      try {
        this.result = await CleanerService.executeWithProgress(categories, (p) => {
          this.progress = p;
        }, 'cleaner');
        this.log = this.result.log;
        void useHistoryStore().load({ reportError: false });
      } catch (error) {
        if (!isCancelledError(error)) app.reportError(error);
      } finally {
        this.loading = false;
        this.activePreset = null;
        this.progress = null;
        app.setBusy(PAGE_IDS.cleaner, false);
      }
    },
    async cancel() {
      await CleanerService.cancel();
    },
  },
});

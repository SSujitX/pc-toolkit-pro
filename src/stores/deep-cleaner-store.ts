import { defineStore } from 'pinia';
import {
  CleanerService,
  type CleanupProgress,
  type CleanupResult,
  type CleanupScanItem,
} from '@/lib/services/api-services';
import type { CleanerCategory } from '@/lib/models/actions';
import { PAGE_IDS } from '@/lib/models/application-shell';
import { isCancelledError } from '@/lib/utils/errors';
import { useAppStore } from './app-store';
import { useHistoryStore } from './history-store';

export const useDeepCleanerStore = defineStore('deepCleaner', {
  state: () => ({
    items: [] as CleanupScanItem[],
    isAdmin: false,
    loading: false,
    progress: null as CleanupProgress | null,
    result: null as CleanupResult | null,
    log: [] as string[],
  }),
  actions: {
    async scan() {
      const app = useAppStore();
      app.setBusy(PAGE_IDS.deepCleaner, true);
      this.loading = true;
      this.result = null;
      this.progress = null;
      try {
        const scan = await CleanerService.scanWithProgress((p) => {
          this.progress = p;
        });
        this.items = scan.items;
        this.isAdmin = scan.isAdmin;
      } catch (error) {
        if (!isCancelledError(error)) app.reportError(error);
      } finally {
        this.loading = false;
        this.progress = null;
        app.setBusy(PAGE_IDS.deepCleaner, false);
      }
    },
    toggle(id: CleanerCategory) {
      this.items = this.items.map((item) =>
        item.id === id ? { ...item, selected: !item.selected } : item
      );
    },
    setSelected(id: CleanerCategory, selected: boolean) {
      this.items = this.items.map((item) => (item.id === id ? { ...item, selected } : item));
    },
    selectAll(selected: boolean) {
      this.items = this.items.map((item) => ({ ...item, selected }));
    },
    async execute() {
      const app = useAppStore();
      const categories = this.items.filter((i) => i.selected).map((i) => i.id);
      if (!categories.length) return;
      app.setBusy(PAGE_IDS.deepCleaner, true);
      this.loading = true;
      this.progress = null;
      try {
        this.result = await CleanerService.executeWithProgress(categories, (p) => {
          this.progress = p;
        }, 'deepCleaner');
        this.log = this.result.log;
        void useHistoryStore().load({ reportError: false });
      } catch (error) {
        if (!isCancelledError(error)) app.reportError(error);
      } finally {
        this.loading = false;
        this.progress = null;
        app.setBusy(PAGE_IDS.deepCleaner, false);
      }
    },
    async cancel() {
      await CleanerService.cancel();
    },
  },
});

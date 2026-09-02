import { defineStore } from 'pinia';
import {
  CleanerService,
  type CleanupProgress,
  type CleanupResult,
  type CleanupScanItem,
} from '@/lib/services/api-services';
import type { CleanerCategory } from '@/lib/models/actions';
import { PAGE_IDS } from '@/lib/models/application-shell';
import { useAppStore } from './app-store';

export const useCleanerStore = defineStore('cleaner', {
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
      app.setBusy(PAGE_IDS.cleaner, true);
      this.loading = true;
      this.result = null;
      try {
        const scan = await CleanerService.scan();
        this.items = scan.items;
        this.isAdmin = scan.isAdmin;
      } catch (error) {
        app.reportError(error);
      } finally {
        this.loading = false;
        app.setBusy(PAGE_IDS.cleaner, false);
      }
    },
    toggle(id: CleanerCategory) {
      this.items = this.items.map((item) =>
        item.id === id ? { ...item, selected: !item.selected } : item
      );
    },
    async execute() {
      const app = useAppStore();
      const categories = this.items.filter((i) => i.selected).map((i) => i.id);
      if (!categories.length) return;
      app.setBusy(PAGE_IDS.cleaner, true);
      this.loading = true;
      this.progress = null;
      try {
        this.result = await CleanerService.executeWithProgress(categories, (p) => {
          this.progress = p;
        });
        this.log = this.result.log;
      } catch (error) {
        app.reportError(error);
      } finally {
        this.loading = false;
        this.progress = null;
        app.setBusy(PAGE_IDS.cleaner, false);
      }
    },
    async cancel() {
      await CleanerService.cancel();
    },
  },
});

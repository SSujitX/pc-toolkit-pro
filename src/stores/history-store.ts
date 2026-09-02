import { defineStore } from 'pinia';
import { HistoryService } from '@/lib/services/api-services';
import type { HistoryRecord } from '@/lib/models/history';
import { PAGE_IDS } from '@/lib/models/application-shell';
import { useAppStore } from './app-store';

export const useHistoryStore = defineStore('history', {
  state: () => ({
    records: [] as HistoryRecord[],
    loading: false,
  }),
  actions: {
    async load(options?: { reportError?: boolean }) {
      const app = useAppStore();
      this.loading = true;
      try {
        this.records = await HistoryService.list();
      } catch (error) {
        if (options?.reportError !== false) app.reportError(error);
      } finally {
        this.loading = false;
      }
    },
    async clear() {
      const app = useAppStore();
      app.setBusy(PAGE_IDS.history, true);
      this.loading = true;
      try {
        await HistoryService.clear();
        this.records = [];
      } catch (error) {
        app.reportError(error);
      } finally {
        this.loading = false;
        app.setBusy(PAGE_IDS.history, false);
      }
    },
  },
});

import { defineStore } from 'pinia';
import {
  SystemInfoService,
  type SystemInformation,
  type SystemInfoProgress,
} from '@/lib/services/api-services';
import { PAGE_IDS } from '@/lib/models/application-shell';
import { useAppStore } from './app-store';

export const useSystemInfoStore = defineStore('systemInfo', {
  state: () => ({
    info: null as SystemInformation | null,
    loading: false,
    copied: false,
    error: null as string | null,
    progress: null as SystemInfoProgress | null,
  }),
  actions: {
    async load() {
      const app = useAppStore();
      app.setBusy(PAGE_IDS.information, true);
      this.loading = true;
      this.error = null;
      this.progress = {
        phase: 'metrics',
        current: 0,
        total: 4,
        message: '',
      };
      try {
        this.info = await SystemInfoService.loadWithProgress((progress) => {
          this.progress = progress;
        });
      } catch (error) {
        this.info = null;
        const message =
          error instanceof Error
            ? error.message
            : typeof error === 'string'
              ? error
              : 'Failed to load system information';
        this.error = message;
        app.reportError(error);
      } finally {
        this.loading = false;
        this.progress = null;
        app.setBusy(PAGE_IDS.information, false);
      }
    },
    async copy() {
      if (!this.info?.copyText) return;
      await navigator.clipboard.writeText(this.info.copyText);
      this.copied = true;
      window.setTimeout(() => {
        this.copied = false;
      }, 2000);
    },
  },
});

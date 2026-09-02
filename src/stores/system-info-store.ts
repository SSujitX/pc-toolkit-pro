import { defineStore } from 'pinia';
import { SystemInfoService, type SystemInformation } from '@/lib/services/api-services';
import { PAGE_IDS } from '@/lib/models/application-shell';
import { useAppStore } from './app-store';

export const useSystemInfoStore = defineStore('systemInfo', {
  state: () => ({
    info: null as SystemInformation | null,
    loading: false,
    copied: false,
  }),
  actions: {
    async load() {
      const app = useAppStore();
      app.setBusy(PAGE_IDS.information, true);
      this.loading = true;
      try {
        this.info = await SystemInfoService.load();
      } catch (error) {
        app.reportError(error);
      } finally {
        this.loading = false;
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

import { defineStore } from 'pinia';
import { PAGE_IDS, type PageId, createSidebarLayoutState } from '@/lib/models/application-shell';

export type ThemeMode = 'light' | 'dark';

export const useAppStore = defineStore('app', {
  state: () => ({
    currentPage: PAGE_IDS.monitor as PageId,
    theme: 'dark' as ThemeMode,
    sidebarExpanded: createSidebarLayoutState(
      typeof window !== 'undefined' ? window.innerWidth : 1280
    ).expanded,
    busyPages: [] as PageId[],
    lastError: null as string | null,
  }),
  actions: {
    loadSettings() {
      const theme = (localStorage.getItem('pt-theme') as ThemeMode | null) ?? 'dark';
      this.setTheme(theme);
      const expanded = localStorage.getItem('pt-sidebar-expanded');
      if (expanded != null) this.sidebarExpanded = expanded === '1';
    },
    setTheme(theme: ThemeMode) {
      this.theme = theme;
      document.documentElement.dataset.theme = theme;
      localStorage.setItem('pt-theme', theme);
    },
    navigate(page: PageId) {
      if (this.busyPages.includes(this.currentPage) && page !== this.currentPage) {
        return;
      }
      this.currentPage = page;
    },
    toggleSidebar() {
      this.sidebarExpanded = !this.sidebarExpanded;
      localStorage.setItem('pt-sidebar-expanded', this.sidebarExpanded ? '1' : '0');
    },
    setBusy(page: PageId, busy: boolean) {
      if (busy && !this.busyPages.includes(page)) this.busyPages.push(page);
      if (!busy) this.busyPages = this.busyPages.filter((p) => p !== page);
    },
    reportError(error: unknown) {
      this.lastError = error instanceof Error ? error.message : String(error);
    },
  },
});

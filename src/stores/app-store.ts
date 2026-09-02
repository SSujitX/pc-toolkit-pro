import { defineStore } from 'pinia';
import { PAGE_IDS, type PageId, createSidebarLayoutState } from '@/lib/models/application-shell';

export type ThemeMode = 'light' | 'dark' | 'system';

function resolveTheme(theme: ThemeMode): 'light' | 'dark' {
  if (theme === 'system') {
    return window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light';
  }
  return theme;
}

export const useAppStore = defineStore('app', {
  state: () => ({
    currentPage: PAGE_IDS.monitor as PageId,
    theme: 'light' as ThemeMode,
    /** When true, sidebar stays expanded. When false, rail + hover peek. */
    sidebarPinned: createSidebarLayoutState(
      typeof window !== 'undefined' ? window.innerWidth : 1280
    ).expanded,
    /** Temporary expand while hovering an unpinned rail. */
    sidebarPeeking: false,
    busyPages: [] as PageId[],
    lastError: null as string | null,
    mediaUnbind: null as (() => void) | null,
  }),
  getters: {
    sidebarExpanded(state): boolean {
      return state.sidebarPinned || state.sidebarPeeking;
    },
  },
  actions: {
    loadSettings() {
      const raw = localStorage.getItem('pt-theme') as ThemeMode | null;
      const theme =
        raw === 'light' || raw === 'dark' || raw === 'system' ? raw : 'light';
      this.setTheme(theme);
      const pinned = localStorage.getItem('pt-sidebar-expanded');
      if (pinned != null) this.sidebarPinned = pinned === '1';
      else this.sidebarPinned = true;
      this.sidebarPeeking = false;
    },
    setTheme(theme: ThemeMode) {
      this.theme = theme;
      localStorage.setItem('pt-theme', theme);
      this.applyResolvedTheme();
      this.bindSystemTheme();
    },
    applyResolvedTheme() {
      document.documentElement.dataset.theme = resolveTheme(this.theme);
    },
    bindSystemTheme() {
      this.mediaUnbind?.();
      this.mediaUnbind = null;
      if (this.theme !== 'system') return;
      const mq = window.matchMedia('(prefers-color-scheme: dark)');
      const onChange = () => this.applyResolvedTheme();
      mq.addEventListener('change', onChange);
      this.mediaUnbind = () => mq.removeEventListener('change', onChange);
    },
    navigate(page: PageId) {
      if (this.busyPages.includes(this.currentPage) && page !== this.currentPage) {
        return;
      }
      this.currentPage = page;
    },
    /** Click the brand toggle: pin open or collapse to rail. */
    toggleSidebarPin() {
      this.sidebarPinned = !this.sidebarPinned;
      this.sidebarPeeking = false;
      localStorage.setItem('pt-sidebar-expanded', this.sidebarPinned ? '1' : '0');
    },
    /** @deprecated use toggleSidebarPin */
    toggleSidebar() {
      this.toggleSidebarPin();
    },
    setSidebarPeek(peeking: boolean) {
      if (this.sidebarPinned) {
        this.sidebarPeeking = false;
        return;
      }
      this.sidebarPeeking = peeking;
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

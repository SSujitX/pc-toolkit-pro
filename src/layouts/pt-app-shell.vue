<script setup lang="ts">
import { computed, defineAsyncComponent, onMounted, watch } from 'vue';
import { useAppStore } from '@/stores/app-store';
import { useMonitorStore } from '@/stores/monitor-store';
import { PAGE_IDS, type PageId } from '@/lib/models/application-shell';
import PtSidebar from './components/pt-sidebar.vue';
import PtWindowTitlebar from './components/pt-window-titlebar.vue';
import MonitorPage from '@/pages/monitor/index.vue';

const CleanerPage = defineAsyncComponent(() => import('@/pages/cleaner/index.vue'));
const PowerPage = defineAsyncComponent(() => import('@/pages/power/index.vue'));
const InformationPage = defineAsyncComponent(() => import('@/pages/information/index.vue'));
const SettingsPage = defineAsyncComponent(() => import('@/pages/settings/index.vue'));

const app = useAppStore();
const monitor = useMonitorStore();

const pageMap: Partial<Record<PageId, unknown>> = {
  [PAGE_IDS.monitor]: MonitorPage,
  [PAGE_IDS.cleaner]: CleanerPage,
  [PAGE_IDS.power]: PowerPage,
  [PAGE_IDS.information]: InformationPage,
  [PAGE_IDS.settings]: SettingsPage,
};

const current = computed(() => pageMap[app.currentPage] ?? MonitorPage);

onMounted(() => {
  monitor.startPolling();
  // Idle preload secondary pages
  window.setTimeout(() => {
    void import('@/pages/cleaner/index.vue');
    void import('@/pages/power/index.vue');
    void import('@/pages/information/index.vue');
    void import('@/pages/settings/index.vue');
  }, 1500);
});

watch(
  () => app.sidebarExpanded,
  (expanded) => {
    document.documentElement.style.setProperty(
      '--sidebar-width',
      expanded
        ? 'var(--layout-sidebar-expanded-width)'
        : 'var(--layout-sidebar-collapsed-width)'
    );
  },
  { immediate: true }
);
</script>

<template>
  <div class="shell">
    <PtSidebar
      :current-page="app.currentPage"
      :busy-pages="app.busyPages"
      :expanded="app.sidebarExpanded"
      @navigate="app.navigate"
      @toggle="app.toggleSidebar"
    />
    <main class="workspace">
      <PtWindowTitlebar />
      <component :is="current" />
    </main>
  </div>
</template>

<style scoped>
.shell {
  display: flex;
  width: 100%;
  height: 100%;
  background: var(--background);
  color: var(--foreground);
}
.workspace {
  position: relative;
  flex: 1;
  min-width: 0;
  background: var(--workspace);
}
</style>

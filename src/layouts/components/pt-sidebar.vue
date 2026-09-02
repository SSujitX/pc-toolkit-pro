<script setup lang="ts">
import { useI18n } from 'vue-i18n';
import {
  ChevronLeft,
  ChevronRight,
  Gauge,
  Info,
  Paintbrush,
  Power,
  Settings,
} from '@lucide/vue';
import { APP_NAME, PRIMARY_NAV, SECONDARY_NAV, type PageId } from '@/lib/models/application-shell';
import appLogo from '@/assets/brand/logo.png';

const { t } = useI18n();
const props = defineProps<{
  currentPage: PageId;
  busyPages: PageId[];
  expanded: boolean;
}>();
const emit = defineEmits<{ navigate: [page: PageId]; toggle: [] }>();

const iconMap = {
  gauge: Gauge,
  broom: Paintbrush,
  power: Power,
  info: Info,
  settings: Settings,
} as const;

function isBusy(page: PageId) {
  return props.busyPages.includes(page);
}
</script>

<template>
  <aside class="sidebar" :class="{ expanded }">
    <div class="brand" data-tauri-drag-region>
      <div class="brand-mark" aria-hidden="true">
        <img :src="appLogo" :alt="APP_NAME" class="brand-logo" width="44" height="44" />
      </div>
      <div class="brand-text">
        <strong>{{ APP_NAME }}</strong>
      </div>
    </div>

    <nav class="nav-scroll">
      <div class="nav-group">
        <div class="nav-group-label">{{ t('navigation.systemGroup') }}</div>
        <div class="nav-group-items">
          <button
            v-for="item in PRIMARY_NAV"
            :key="item.id"
            type="button"
            class="nav-item"
            :class="{ active: currentPage === item.id }"
            :title="expanded ? undefined : t(`navigation.${item.id}`)"
            @click="emit('navigate', item.id)"
          >
            <span class="nav-icon">
              <component :is="iconMap[item.icon]" :size="20" :stroke-width="1.9" />
              <span v-if="isBusy(item.id)" class="nav-busy" />
            </span>
            <span class="nav-label">{{ t(`navigation.${item.id}`) }}</span>
          </button>
        </div>
      </div>
    </nav>

    <div class="sidebar-footer">
      <button
        v-for="item in SECONDARY_NAV"
        :key="item.id"
        type="button"
        class="nav-item"
        :class="{ active: currentPage === item.id }"
        :title="expanded ? undefined : t(`navigation.${item.id}`)"
        @click="emit('navigate', item.id)"
      >
        <span class="nav-icon">
          <component :is="iconMap[item.icon]" :size="20" :stroke-width="1.9" />
        </span>
        <span class="nav-label">{{ t(`navigation.${item.id}`) }}</span>
      </button>
      <button
        type="button"
        class="nav-item sidebar-toggle"
        @click="emit('toggle')"
      >
        <span class="nav-icon">
          <ChevronLeft v-if="expanded" :size="18" :stroke-width="2" />
          <ChevronRight v-else :size="18" :stroke-width="2" />
        </span>
        <span class="nav-label">{{
          expanded ? t('common.collapseSidebar') : t('common.expandSidebar')
        }}</span>
      </button>
    </div>
  </aside>
</template>

<style scoped>
.sidebar {
  display: flex;
  width: var(--sidebar-width);
  min-width: var(--sidebar-width);
  height: 100%;
  flex-direction: column;
  background: var(--sidebar);
  color: var(--sidebar-foreground);
  border-right: 1px solid color-mix(in oklab, var(--border) 80%, transparent);
  transition:
    width var(--sidebar-transition-duration) var(--sidebar-transition-easing),
    min-width var(--sidebar-transition-duration) var(--sidebar-transition-easing);
}
.sidebar.expanded {
  --sidebar-width: var(--layout-sidebar-expanded-width);
}
.sidebar:not(.expanded) {
  --sidebar-width: var(--layout-sidebar-collapsed-width);
}
.brand {
  display: flex;
  height: var(--layout-sidebar-brand-height);
  align-items: center;
  gap: 0;
  padding-inline: 14px;
}
.brand-mark {
  display: grid;
  width: 44px;
  height: 44px;
  place-items: center;
  flex: none;
  overflow: hidden;
  border-radius: 14px;
  background: transparent;
  box-shadow: 0 8px 18px -10px color-mix(in oklab, var(--primary) 55%, transparent);
}
.brand-logo {
  width: 44px;
  height: 44px;
  object-fit: cover;
  display: block;
}
.brand-text {
  max-width: 0;
  overflow: hidden;
  opacity: 0;
  white-space: nowrap;
}
.brand-text strong {
  font-size: 16px;
  font-weight: 700;
  letter-spacing: -0.03em;
}
.sidebar.expanded .brand {
  gap: 12px;
  padding-inline: 18px;
}
.sidebar.expanded .brand-text {
  max-width: 160px;
  opacity: 1;
}
.nav-scroll {
  display: flex;
  flex-direction: column;
  gap: 14px;
  padding: 4px 10px;
  overflow: auto;
}
.nav-group-label {
  max-height: 0;
  margin: 0;
  padding-inline: 12px;
  overflow: hidden;
  opacity: 0;
  color: color-mix(in oklab, var(--sidebar-foreground) 55%, transparent);
  font-size: 11px;
  font-weight: 650;
  letter-spacing: 0.06em;
  text-transform: uppercase;
}
.sidebar.expanded .nav-group-label {
  max-height: 20px;
  margin-bottom: 6px;
  opacity: 1;
}
.nav-group-items {
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.nav-item {
  position: relative;
  display: flex;
  width: 100%;
  height: var(--layout-sidebar-item-height);
  align-items: center;
  gap: 0;
  border: 0;
  border-radius: 12px;
  padding: 0 12px;
  background: transparent;
  color: inherit;
  cursor: pointer;
}
.sidebar.expanded .nav-item {
  gap: 12px;
  padding-inline: 12px;
}
.nav-item:hover:not(.active) {
  background: color-mix(in oklab, var(--sidebar-accent) 55%, transparent);
}
.nav-item.active {
  background: var(--sidebar-accent);
  color: var(--sidebar-accent-foreground);
  font-weight: 650;
}
.nav-item.active::before {
  content: '';
  position: absolute;
  left: 0;
  width: 3px;
  height: 22px;
  border-radius: 999px;
  background: var(--primary);
}
.nav-icon {
  position: relative;
  display: grid;
  width: 24px;
  height: 24px;
  place-items: center;
  flex: none;
}
.nav-busy {
  position: absolute;
  inset: -4px;
  border: 1.5px solid color-mix(in oklab, var(--primary) 20%, transparent);
  border-top-color: var(--primary);
  border-radius: 50%;
  animation: icon-spin 0.85s linear infinite;
}
.nav-label {
  max-width: 0;
  overflow: hidden;
  opacity: 0;
  white-space: nowrap;
  font-size: 14px;
}
.sidebar.expanded .nav-label {
  max-width: 160px;
  opacity: 1;
}
.sidebar-footer {
  margin-top: auto;
  display: flex;
  flex-direction: column;
  gap: 2px;
  padding: 10px;
}
.sidebar-toggle {
  color: color-mix(in oklab, var(--sidebar-foreground) 70%, transparent);
  font-size: 12px;
}
</style>

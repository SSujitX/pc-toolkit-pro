<script setup lang="ts">
import { computed } from 'vue';
import { useI18n } from 'vue-i18n';
import {
  Brain,
  Gauge,
  History,
  Info,
  Layers,
  Paintbrush,
  PanelLeftClose,
  PanelLeftOpen,
  Power,
  Settings,
} from '@lucide/vue';
import {
  APP_NAME,
  PRIMARY_NAV_GROUPS,
  SECONDARY_NAV,
  type NavIconId,
  type PageId,
} from '@/lib/models/application-shell';
import appLogo from '@/assets/brand/logo.png';

const { t } = useI18n();
const props = defineProps<{
  currentPage: PageId;
  busyPages: PageId[];
  pinned: boolean;
  peeking: boolean;
}>();
const emit = defineEmits<{
  navigate: [page: PageId];
  togglePin: [];
  peek: [value: boolean];
}>();

const iconMap: Record<NavIconId, unknown> = {
  gauge: Gauge,
  broom: Paintbrush,
  layers: Layers,
  brain: Brain,
  power: Power,
  info: Info,
  history: History,
  settings: Settings,
};

const visuallyExpanded = computed(() => props.pinned || props.peeking);
const toggleLabel = computed(() =>
  props.pinned ? t('common.collapseSidebar') : t('common.expandSidebar')
);

function isBusy(page: PageId) {
  return props.busyPages.includes(page);
}

function onEnter() {
  emit('peek', true);
}

function onLeave() {
  emit('peek', false);
}
</script>

<template>
  <aside
    class="sidebar"
    :class="{
      expanded: visuallyExpanded,
      pinned,
      peeking: peeking && !pinned,
    }"
    @mouseenter="onEnter"
    @mouseleave="onLeave"
  >
    <div class="brand-block">
      <div class="brand" data-tauri-drag-region>
        <div class="brand-mark" aria-hidden="true">
          <img :src="appLogo" :alt="APP_NAME" class="brand-logo" width="44" height="44" />
        </div>
        <div class="brand-text">
          <strong>{{ APP_NAME }}</strong>
        </div>
        <button
          v-if="visuallyExpanded"
          type="button"
          class="sidebar-toggle"
          :aria-label="toggleLabel"
          :aria-expanded="pinned"
          :title="toggleLabel"
          data-tauri-drag-region="false"
          @click.stop="emit('togglePin')"
        >
          <PanelLeftClose v-if="pinned" :size="18" :stroke-width="2" />
          <PanelLeftOpen v-else :size="18" :stroke-width="2" />
        </button>
      </div>

      <button
        v-if="!visuallyExpanded"
        type="button"
        class="sidebar-toggle sidebar-toggle--rail"
        :aria-label="toggleLabel"
        :aria-expanded="false"
        :title="toggleLabel"
        @click.stop="emit('togglePin')"
      >
        <PanelLeftOpen :size="18" :stroke-width="2" />
      </button>
    </div>

    <nav class="nav-scroll">
      <div v-for="group in PRIMARY_NAV_GROUPS" :key="group.id" class="nav-group">
        <div class="nav-group-label">{{ t(group.titleKey) }}</div>
        <div class="nav-group-items">
          <button
            v-for="item in group.items"
            :key="item.id"
            type="button"
            class="nav-item"
            :class="{ active: currentPage === item.id }"
            :title="visuallyExpanded ? undefined : t(`navigation.${item.id}`)"
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
        :title="visuallyExpanded ? undefined : t(`navigation.${item.id}`)"
        @click="emit('navigate', item.id)"
      >
        <span class="nav-icon">
          <component :is="iconMap[item.icon]" :size="20" :stroke-width="1.9" />
        </span>
        <span class="nav-label">{{ t(`navigation.${item.id}`) }}</span>
      </button>
    </div>
  </aside>
</template>

<style scoped>
.sidebar {
  position: relative;
  z-index: 30;
  display: flex;
  width: 100%;
  min-width: 0;
  height: 100%;
  flex-direction: column;
  background: var(--sidebar);
  color: var(--sidebar-foreground);
  border-right: 1px solid color-mix(in oklab, var(--border) 80%, transparent);
}

.brand-block {
  display: flex;
  flex: none;
  flex-direction: column;
  align-items: stretch;
  gap: 6px;
  padding-bottom: 4px;
}
.brand {
  display: flex;
  height: var(--layout-sidebar-brand-height);
  align-items: center;
  gap: 0;
  padding-inline: 12px;
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
  gap: 10px;
  padding-inline: 14px 10px;
}
.sidebar.expanded .brand-text {
  max-width: 140px;
  flex: 1;
  opacity: 1;
}

.sidebar-toggle {
  display: grid;
  width: 32px;
  height: 32px;
  flex: none;
  place-items: center;
  margin-inline-start: auto;
  border: 1px solid color-mix(in oklab, var(--border) 70%, transparent);
  border-radius: 10px;
  background: transparent;
  color: color-mix(in oklab, var(--sidebar-foreground) 72%, transparent);
  cursor: pointer;
}
.sidebar-toggle:hover {
  background: color-mix(in oklab, var(--sidebar-accent) 55%, transparent);
  color: var(--sidebar-foreground);
}
.sidebar-toggle--rail {
  width: 40px;
  height: 32px;
  margin: 0 auto;
}

.nav-scroll {
  display: flex;
  flex-direction: column;
  gap: 18px;
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
</style>

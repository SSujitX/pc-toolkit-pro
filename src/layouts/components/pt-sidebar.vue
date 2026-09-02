<script setup lang="ts">
import { useI18n } from 'vue-i18n';
import { APP_NAME, PRIMARY_NAV, SECONDARY_NAV, type PageId } from '@/lib/models/application-shell';

const { t } = useI18n();
const props = defineProps<{
  currentPage: PageId;
  busyPages: PageId[];
  expanded: boolean;
}>();
const emit = defineEmits<{ navigate: [page: PageId]; toggle: [] }>();

function isBusy(page: PageId) {
  return props.busyPages.includes(page);
}

const iconGlyph: Record<string, string> = {
  gauge: '▣',
  broom: '▤',
  power: '⚡',
  info: 'ℹ',
  settings: '⚙',
};
</script>

<template>
  <aside class="sidebar" :class="{ expanded }">
    <div class="brand" data-tauri-drag-region>
      <div class="brand-icon">PT</div>
      <strong>{{ APP_NAME }}</strong>
    </div>
    <nav class="nav-list">
      <div class="nav-group">
        <div class="nav-group-label">{{ t('navigation.systemGroup') }}</div>
        <div class="nav-group-items">
          <button
            v-for="item in PRIMARY_NAV"
            :key="item.id"
            type="button"
            class="nav-item"
            :class="{ active: currentPage === item.id }"
            @click="emit('navigate', item.id)"
          >
            <span class="nav-icon">
              {{ iconGlyph[item.icon] }}
              <span v-if="isBusy(item.id)" class="nav-icon-status" />
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
        @click="emit('navigate', item.id)"
      >
        <span class="nav-icon">{{ iconGlyph[item.icon] }}</span>
        <span class="nav-label">{{ t(`navigation.${item.id}`) }}</span>
      </button>
      <button type="button" class="nav-item sidebar-toggle" @click="emit('toggle')">
        <span class="nav-icon">{{ expanded ? '‹' : '›' }}</span>
        <span class="nav-label">{{
          expanded ? t('common.collapseSidebar') : t('common.expandSidebar')
        }}</span>
      </button>
    </div>
  </aside>
</template>

<style scoped>
@reference "@assets/main.css";
.sidebar {
  display: flex;
  width: var(--sidebar-width);
  min-width: var(--sidebar-width);
  height: 100%;
  flex-direction: column;
  background: var(--sidebar);
  color: var(--sidebar-foreground);
  border-right: 1px solid var(--border);
  transition: width var(--sidebar-transition-duration) var(--sidebar-transition-easing),
    min-width var(--sidebar-transition-duration) var(--sidebar-transition-easing);
}
.sidebar.expanded {
  --sidebar-width: var(--layout-sidebar-expanded-width);
}
.brand {
  display: flex;
  height: var(--layout-sidebar-brand-height);
  align-items: center;
  gap: 0;
  padding-inline: 12px;
}
.brand-icon {
  display: grid;
  width: 40px;
  height: 40px;
  place-items: center;
  border-radius: 10px;
  background: var(--primary);
  color: var(--primary-foreground);
  font-weight: 700;
  font-size: 13px;
}
.brand strong {
  max-width: 0;
  overflow: hidden;
  opacity: 0;
  white-space: nowrap;
  font-size: 15px;
  letter-spacing: -0.02em;
}
.sidebar.expanded .brand {
  gap: 10px;
  padding-inline: 16px;
}
.sidebar.expanded .brand strong {
  max-width: 160px;
  opacity: 1;
}
.nav-list {
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 4px 8px;
}
.nav-group-label {
  max-height: 0;
  overflow: hidden;
  opacity: 0;
  padding-inline: 12px;
  font-size: 11px;
  font-weight: 600;
  letter-spacing: 0.04em;
  text-transform: uppercase;
  color: color-mix(in oklab, var(--sidebar-foreground) 55%, transparent);
}
.sidebar.expanded .nav-group-label {
  max-height: 20px;
  opacity: 1;
  margin-bottom: 4px;
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
  border-radius: 8px;
  padding: 0 14px;
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
  font-weight: 600;
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
.nav-icon-status {
  position: absolute;
  inset: -3px;
  border: 1.5px solid color-mix(in oklab, var(--primary) 25%, transparent);
  border-top-color: var(--primary);
  border-radius: 50%;
  animation: spin 0.9s linear infinite;
}
.nav-label {
  max-width: 0;
  overflow: hidden;
  opacity: 0;
  white-space: nowrap;
  font-size: 14px;
}
.sidebar.expanded .nav-label {
  max-width: 150px;
  opacity: 1;
}
.sidebar-footer {
  margin-top: auto;
  display: flex;
  flex-direction: column;
  gap: 2px;
  padding: 8px;
}
@keyframes spin {
  to {
    transform: rotate(360deg);
  }
}
</style>

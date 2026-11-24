<script setup lang="ts">
import { computed, ref, watch } from 'vue';
import type { Component } from 'vue';
import { useI18n } from 'vue-i18n';
import {
  CalendarClock,
  Database,
  File,
  Flame,
  Folder,
  Link2,
  Rocket,
  ServerCog,
  ShieldAlert,
} from '@lucide/vue';
import PtConfirmDialog from '@/components/custom/pt-confirm-dialog.vue';
import PtMiddleEllipsis from '@/components/custom/pt-middle-ellipsis.vue';
import { formatBytes } from '@/lib/utils/format';
import {
  LEFTOVER_GROUPS,
  leftoverGroup,
  type LeftoverGroup,
  type LeftoverItem,
  type LeftoverKind,
  type LeftoverScan,
} from '@/lib/models/uninstaller';

const props = defineProps<{
  scans: LeftoverScan[];
  forced: boolean;
  isAdmin: boolean;
  selectedCount: number;
  selectedBytes: number;
  needsAdmin: boolean;
}>();

const emit = defineEmits<{
  toggle: [scanId: string, itemId: string, selected: boolean];
  selectGroup: [group: LeftoverGroup | 'all', selected: boolean];
  clean: [];
  skip: [];
  restartAdmin: [];
}>();

const { t } = useI18n();
const activeGroup = ref<LeftoverGroup | 'all'>('all');
const confirmOpen = ref(false);

const kindIcons: Record<LeftoverKind, Component> = {
  folder: Folder,
  file: File,
  registryKey: Database,
  registryValue: Database,
  shortcut: Link2,
  startupEntry: Rocket,
  service: ServerCog,
  scheduledTask: CalendarClock,
  firewallRule: Flame,
};

const allItems = computed(() => props.scans.flatMap((s) => s.items));
const totalBytes = computed(() => allItems.value.reduce((n, i) => n + i.bytes, 0));

const groupTabs = computed(() => {
  const tabs = LEFTOVER_GROUPS.map((group) => ({
    id: group as LeftoverGroup | 'all',
    label: t(`uninstaller.leftovers.groups.${group}`),
    count: allItems.value.filter((i) => leftoverGroup(i.kind) === group).length,
  })).filter((tab) => tab.count > 0);
  return [{ id: 'all' as const, label: t('uninstaller.filters.all'), count: allItems.value.length }, ...tabs];
});

watch(
  () => groupTabs.value.map((g) => g.id).join(','),
  () => {
    if (!groupTabs.value.some((g) => g.id === activeGroup.value)) activeGroup.value = 'all';
  }
);

function inGroup(item: LeftoverItem): boolean {
  return activeGroup.value === 'all' || leftoverGroup(item.kind) === activeGroup.value;
}

const sections = computed(() =>
  props.scans
    .map((scan) => ({
      scan,
      items: scan.items.filter(inGroup),
    }))
    .filter((section) => section.items.length > 0)
);

const groupItems = computed(() => allItems.value.filter(inGroup));
const groupAllSelected = computed(
  () => groupItems.value.length > 0 && groupItems.value.every((i) => i.selected)
);

const heading = computed(() =>
  props.forced ? t('uninstaller.leftovers.forcedTitle') : t('uninstaller.leftovers.title')
);

/** Startup entries are identified by their value name; everything else by its location. */
function itemLabel(item: LeftoverItem): string {
  return item.kind === 'startupEntry' && item.detail ? item.detail : item.location;
}

function itemDetail(item: LeftoverItem): string | null {
  if (item.kind === 'startupEntry') return item.location;
  return item.detail;
}

function onClean() {
  confirmOpen.value = true;
}
</script>

<template>
  <div class="review">
    <header class="summary">
      <div class="summary-copy">
        <strong>{{ heading }}</strong>
        <span>
          {{ t('uninstaller.leftovers.summary', { count: allItems.length }, allItems.length) }}
          · {{ t('uninstaller.leftovers.summarySize') }}
          <em>{{ formatBytes(totalBytes) }}</em>
        </span>
      </div>
      <button
        type="button"
        class="text-btn"
        @click="emit('selectGroup', activeGroup, !groupAllSelected)"
      >
        {{ groupAllSelected ? t('uninstaller.leftovers.deselectAll') : t('uninstaller.leftovers.selectAll') }}
      </button>
    </header>

    <nav class="tabs" role="tablist">
      <button
        v-for="tab in groupTabs"
        :key="tab.id"
        type="button"
        role="tab"
        class="tab"
        :class="{ active: activeGroup === tab.id }"
        :aria-selected="activeGroup === tab.id"
        @click="activeGroup = tab.id"
      >
        {{ tab.label }}
        <span class="tab-count">{{ tab.count }}</span>
      </button>
    </nav>

    <div class="list-card">
      <section v-for="section in sections" :key="section.scan.scanId" class="section">
        <h3 v-if="scans.length > 1">{{ section.scan.programName }}</h3>
        <ul>
          <li v-for="item in section.items" :key="`${section.scan.scanId}:${item.id}`" class="row">
            <label class="row-check">
              <input
                type="checkbox"
                :checked="item.selected"
                @change="emit('toggle', section.scan.scanId, item.id, !item.selected)"
              />
              <span class="kind-icon" :data-kind="leftoverGroup(item.kind)" aria-hidden="true">
                <component :is="kindIcons[item.kind]" :size="15" :stroke-width="1.8" />
              </span>
              <span class="row-copy">
                <span class="row-title">
                  <span class="kind-label">{{ t(`uninstaller.leftovers.kinds.${item.kind}`) }}</span>
                  <span
                    v-if="item.confidence === 'medium'"
                    class="badge badge-review"
                    :title="t('uninstaller.leftovers.reviewHint')"
                  >{{ t('uninstaller.leftovers.review') }}</span>
                  <span
                    v-if="item.userData"
                    class="badge badge-user-data"
                    :title="t('uninstaller.leftovers.userDataHint')"
                  >{{ t('uninstaller.leftovers.userData') }}</span>
                  <span
                    v-if="item.requiresAdmin && !isAdmin"
                    class="badge badge-admin"
                    :title="t('uninstaller.leftovers.adminHint')"
                  >{{ t('uninstaller.leftovers.admin') }}</span>
                </span>
                <PtMiddleEllipsis class="row-path" :text="itemLabel(item)" />
                <small v-if="itemDetail(item)" class="row-detail" :title="itemDetail(item) ?? ''">
                  {{ itemDetail(item) }}
                </small>
              </span>
            </label>
            <span class="row-size">{{ item.bytes ? formatBytes(item.bytes) : '' }}</span>
          </li>
        </ul>
      </section>
    </div>

    <footer class="action-bar">
      <div class="action-copy">
        <strong>{{ t('uninstaller.leftovers.footerSelected', { count: selectedCount }, selectedCount) }}</strong>
        <span v-if="selectedBytes">{{ formatBytes(selectedBytes) }}</span>
        <span v-if="needsAdmin" class="admin-note">
          <ShieldAlert :size="14" :stroke-width="1.9" aria-hidden="true" />
          {{ t('uninstaller.leftovers.adminNotice') }}
          <button type="button" class="text-btn" @click="emit('restartAdmin')">
            {{ t('uninstaller.leftovers.restartAdmin') }}
          </button>
        </span>
      </div>
      <div class="action-buttons">
        <button type="button" class="pt-btn" @click="emit('skip')">
          {{ t('uninstaller.leftovers.skip') }}
        </button>
        <button
          type="button"
          class="pt-btn pt-btn-primary"
          :disabled="selectedCount === 0"
          @click="onClean"
        >
          {{ t('uninstaller.leftovers.delete') }}
        </button>
      </div>
    </footer>

    <PtConfirmDialog
      v-model:open="confirmOpen"
      :title="t('uninstaller.leftovers.confirmTitle', { count: selectedCount }, selectedCount)"
      :message="t('uninstaller.leftovers.confirmBody')"
      :confirm-text="t('uninstaller.leftovers.confirmAction')"
      :cancel-text="t('common.cancel')"
      destructive
      @confirm="emit('clean')"
    />
  </div>
</template>

<style scoped>
.review {
  display: flex;
  flex: 1;
  min-height: 0;
  flex-direction: column;
  gap: 10px;
}
.summary {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
  padding: 2px;
}
.summary-copy {
  display: flex;
  flex-wrap: wrap;
  align-items: baseline;
  gap: 10px;
}
.summary-copy strong {
  font-size: 0.9375rem;
}
.summary-copy span {
  color: var(--muted-foreground);
  font-size: 0.8125rem;
}
.summary-copy em {
  color: var(--primary);
  font-size: 1.05rem;
  font-style: normal;
  font-weight: 700;
}
.text-btn {
  padding: 0;
  border: 0;
  background: transparent;
  color: var(--primary);
  font-size: 0.75rem;
  font-weight: 650;
  cursor: pointer;
}
.text-btn:hover {
  text-decoration: underline;
}
.tabs {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
}
.tab {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  height: 30px;
  padding: 0 12px;
  border: 1px solid color-mix(in oklab, var(--border) 75%, transparent);
  border-radius: 999px;
  background: var(--card);
  color: var(--muted-foreground);
  font-size: 0.75rem;
  font-weight: 600;
  cursor: pointer;
}
.tab:hover {
  color: var(--foreground);
  background: color-mix(in oklab, var(--muted) 45%, var(--card));
}
.tab.active {
  border-color: color-mix(in oklab, var(--primary) 40%, var(--border));
  background: color-mix(in oklab, var(--primary) 10%, var(--card));
  color: var(--foreground);
}
.tab-count {
  min-width: 18px;
  padding: 0 5px;
  border-radius: 999px;
  background: var(--muted);
  color: var(--muted-foreground);
  font-size: 0.625rem;
  line-height: 16px;
  text-align: center;
}
.list-card {
  min-height: 0;
  flex: 1;
  overflow: auto;
  border: 1px solid color-mix(in oklab, var(--border) 75%, transparent);
  border-radius: 14px;
  background: var(--card);
  padding: 6px 8px 10px;
}
.section h3 {
  margin: 10px 10px 4px;
  color: var(--muted-foreground);
  font-size: 0.6875rem;
  font-weight: 700;
  letter-spacing: 0.04em;
  text-transform: uppercase;
}
.section ul {
  list-style: none;
  margin: 0;
  padding: 0;
}
.row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  min-height: 48px;
  padding: 6px 10px;
  border-radius: 12px;
}
.row:hover {
  background: color-mix(in oklab, var(--muted) 40%, transparent);
}
.row-check {
  display: flex;
  min-width: 0;
  flex: 1;
  align-items: center;
  gap: 10px;
  cursor: pointer;
}
.row-check input {
  flex: none;
}
.kind-icon {
  display: grid;
  width: 28px;
  height: 28px;
  flex: none;
  place-items: center;
  border-radius: 8px;
  background: color-mix(in oklab, var(--primary) 10%, transparent);
  color: var(--primary);
}
.kind-icon[data-kind='registry'] {
  background: color-mix(in oklab, var(--warning) 14%, transparent);
  color: var(--warning);
}
.kind-icon[data-kind='system'],
.kind-icon[data-kind='startup'] {
  background: color-mix(in oklab, var(--success) 14%, transparent);
  color: var(--success);
}
.row-copy {
  display: flex;
  min-width: 0;
  flex: 1;
  flex-direction: column;
  gap: 1px;
}
.row-title {
  display: flex;
  align-items: center;
  gap: 6px;
}
.kind-label {
  font-size: 0.75rem;
  font-weight: 650;
}
.row-path {
  color: var(--foreground);
}
.row-detail {
  overflow: hidden;
  color: var(--muted-foreground);
  font-size: 0.6875rem;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.badge {
  display: inline-flex;
  padding: 1px 7px;
  border-radius: 999px;
  font-size: 0.625rem;
  font-weight: 700;
}
.badge-review {
  background: color-mix(in oklab, var(--warning) 18%, transparent);
  color: color-mix(in oklab, var(--warning) 85%, var(--foreground));
}
.badge-user-data {
  background: color-mix(in oklab, var(--primary) 12%, transparent);
  color: color-mix(in oklab, var(--primary) 80%, var(--foreground));
}
.badge-admin {
  background: var(--muted);
  color: var(--muted-foreground);
}
.row-size {
  flex: none;
  color: var(--muted-foreground);
  font-size: 0.75rem;
  font-weight: 650;
}
.action-bar {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  padding: 12px 14px;
  border: 1px solid color-mix(in oklab, var(--border) 75%, transparent);
  border-radius: 14px;
  background: var(--card);
  box-shadow: var(--shadow-card);
}
.action-copy {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 4px 10px;
  min-width: 0;
}
.action-copy strong {
  font-size: 0.8125rem;
}
.action-copy > span {
  color: var(--muted-foreground);
  font-size: 0.75rem;
}
.admin-note {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  color: var(--warning) !important;
}
.action-buttons {
  display: flex;
  gap: 8px;
}
</style>

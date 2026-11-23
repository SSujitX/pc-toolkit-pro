<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import { useI18n } from 'vue-i18n';
import { Check, History, PackageX, RefreshCw, Search, Trash2 } from '@lucide/vue';
import PtPageShell from '@/components/custom/pt-page-shell.vue';
import PtOperationWorkspace from '@/components/custom/pt-operation-workspace.vue';
import PtSoftSelect from '@/components/custom/pt-soft-select.vue';
import PtEmptyState from '@/components/custom/pt-empty-state.vue';
import PtUninstallConfirm from '@/pages/uninstaller/pt-uninstall-confirm.vue';
import PtLeftoverReview from '@/pages/uninstaller/pt-leftover-review.vue';
import PtUninstallSummary from '@/pages/uninstaller/pt-uninstall-summary.vue';
import PtUninstallProgressDialog from '@/pages/uninstaller/pt-uninstall-progress-dialog.vue';
import PtProgramIcon from '@/pages/uninstaller/pt-program-icon.vue';
import { formatBytes } from '@/lib/utils/format';
import { SettingsApi } from '@/lib/services/api-services';
import {
  PROGRAM_FILTERS,
  type InstalledProgram,
  type ProgramSort,
  type UninstallOptions,
} from '@/lib/models/uninstaller';
import { useUninstallerStore } from '@/stores/uninstaller-store';
import { useAppStore } from '@/stores/app-store';

const { t, locale } = useI18n();
const store = useUninstallerStore();
const app = useAppStore();

const confirmOpen = ref(false);
/** Programs the confirm dialog is about (a single row action or the current selection). */
const confirmTargets = ref<InstalledProgram[]>([]);

onMounted(() => {
  void store.ensureLoaded();
});

const sortOptions = computed(() =>
  (['name', 'size', 'date', 'publisher'] as ProgramSort[]).map((value) => ({
    value,
    label: t(`uninstaller.sort.${value}`),
  }))
);

const visibleAllSelected = computed(
  () =>
    store.visiblePrograms.length > 0 &&
    store.visiblePrograms.every((p) => store.selectedIdSet.has(p.id))
);

/** Initial program listing uses the page workspace; uninstall / scan / clean use the dialog. */
const showWorkspace = computed(() => store.operation === 'listing' && !store.loaded);
const showDialog = computed(() => store.busy && store.operation !== 'listing');

/* Live loading card: ticking clock + stage-weighted percent (registry 0–5, programs 5–90, Store 90–100). */
const nowMs = ref(Date.now());
let listingClock: number | null = null;
watch(
  showWorkspace,
  (visible) => {
    if (visible && listingClock === null) {
      listingClock = window.setInterval(() => (nowMs.value = Date.now()), 250);
    } else if (!visible && listingClock !== null) {
      window.clearInterval(listingClock);
      listingClock = null;
    }
  },
  { immediate: true }
);
onBeforeUnmount(() => {
  if (listingClock !== null) window.clearInterval(listingClock);
});

const listingPercent = computed(() => {
  const l = store.listing;
  if (!l) return 2;
  if (l.stage === 'registry') return 4;
  if (l.stage === 'desktop') return 5 + Math.round((l.total ? l.current / l.total : 0) * 85);
  if (l.stage === 'store') return 92;
  return 100;
});
const listingSourceLabel = computed(() =>
  t(`uninstaller.listing.stage.${store.listing?.stage ?? 'registry'}`)
);
const listingSourceValue = computed(() => {
  const l = store.listing;
  if (l?.stage === 'desktop' && l.name) return l.name;
  return t(`uninstaller.listing.detail.${l?.stage ?? 'registry'}`);
});
const listingStats = computed(() => {
  const l = store.listing;
  const secs = l ? Math.max(0, Math.floor((nowMs.value - l.startedAt) / 1000)) : 0;
  return [
    { label: t('uninstaller.listing.found'), value: String(l?.found ?? 0) },
    {
      label: t('uninstaller.listing.checked'),
      value: l && l.total ? `${l.current} / ${l.total}` : '—',
    },
    { label: t('uninstaller.dialog.stats.elapsed'), value: secs >= 60 ? `${Math.floor(secs / 60)}m ${secs % 60}s` : `${secs}s` },
  ];
});

function formatDate(iso: string | null): string {
  if (!iso) return t('uninstaller.unknownDate');
  try {
    return new Intl.DateTimeFormat(locale.value || undefined, { dateStyle: 'medium' }).format(
      new Date(`${iso}T00:00:00`)
    );
  } catch {
    return iso;
  }
}

function openConfirm(programs: InstalledProgram[]) {
  if (!programs.length || store.busy) return;
  confirmTargets.value = programs;
  confirmOpen.value = true;
}

function onConfirm(options: UninstallOptions) {
  void store.uninstall(
    confirmTargets.value.map((p) => p.id),
    options
  );
}

function onForce(programId: string) {
  void store.forceRemove(programId);
}

async function openBackups() {
  try {
    await SettingsApi.openAppDataFolder();
  } catch (error) {
    app.reportError(error);
  }
}

function onSort(value: string | number) {
  store.sort = String(value) as ProgramSort;
}
</script>

<template>
  <PtPageShell :title="t('uninstaller.title')" :subtitle="t('uninstaller.subtitle')" content-mode="workspace">
    <template #actions>
      <button
        v-if="store.view === 'list'"
        type="button"
        class="pt-btn"
        :disabled="store.busy"
        @click="store.load()"
      >
        <RefreshCw :size="14" :stroke-width="2" aria-hidden="true" />
        {{ t('uninstaller.refresh') }}
      </button>
    </template>

    <PtOperationWorkspace
      v-if="showWorkspace"
      :status="t('uninstaller.listing.status')"
      :title="t('uninstaller.loadingTitle')"
      :source-label="listingSourceLabel"
      :source-value="listingSourceValue"
      :source-icon="PackageX"
      :progress="listingPercent"
      :stats="listingStats"
      :hint="t('uninstaller.listing.hint')"
      :icon="PackageX"
      :cancellable="false"
    >
      <TransitionGroup v-if="store.listingFeed.length" tag="ul" name="feed" class="listing-feed">
        <li v-for="(item, index) in store.listingFeed" :key="item.id" :style="{ opacity: 1 - index * 0.14 }">
          <PtProgramIcon :name="item.name" :src="store.icons[item.id] ?? null" :size="20" />
          <span class="feed-name">{{ item.name }}</span>
          <Check v-if="index > 0" :size="13" :stroke-width="2.6" class="feed-ok" aria-hidden="true" />
          <span v-else class="feed-dot" aria-hidden="true" />
        </li>
      </TransitionGroup>
    </PtOperationWorkspace>

    <PtLeftoverReview
      v-else-if="store.view === 'leftovers'"
      :scans="store.scans"
      :forced="store.forced"
      :is-admin="store.isAdmin"
      :selected-count="store.selectedLeftovers.length"
      :selected-bytes="store.selectedLeftoverBytes"
      :needs-admin="store.selectedNeedsAdmin"
      @toggle="store.setLeftoverSelectedIn"
      @select-group="store.setGroupSelected"
      @clean="store.cleanSelected()"
      @skip="store.skipLeftovers()"
      @restart-admin="store.restartAsAdministrator()"
    />

    <PtUninstallSummary
      v-else-if="store.view === 'summary'"
      :reports="store.reports"
      :clean-result="store.cleanResult"
      :had-leftovers="store.scans.length > 0"
      :scanned-programs="store.scans.map((scan) => ({ id: scan.programId, name: scan.programName }))"
      @done="store.finish()"
      @open-backups="openBackups"
    />

    <div v-else class="programs">
      <div class="toolbar">
        <label class="search">
          <Search :size="15" :stroke-width="2" aria-hidden="true" />
          <input
            v-model="store.query"
            type="search"
            :placeholder="t('uninstaller.searchPlaceholder')"
            :aria-label="t('uninstaller.searchPlaceholder')"
          />
        </label>
        <label class="sort">
          <span>{{ t('uninstaller.sortLabel') }}</span>
          <PtSoftSelect
            :model-value="store.sort"
            :options="sortOptions"
            :aria-label="t('uninstaller.sortLabel')"
            @update:model-value="onSort"
          />
        </label>
        <label class="runtimes" :title="t('uninstaller.showRuntimesHint')">
          <input v-model="store.showRuntimes" type="checkbox" />
          {{ t('uninstaller.showRuntimes') }}
        </label>
      </div>

      <nav class="filters" role="tablist">
        <button
          v-for="filter in PROGRAM_FILTERS"
          :key="filter"
          type="button"
          role="tab"
          class="chip"
          :class="{ active: store.filter === filter }"
          :aria-selected="store.filter === filter"
          @click="store.filter = filter"
        >
          {{ t(`uninstaller.filters.${filter}`) }}
          <span class="chip-count">{{ store.filterCounts[filter] }}</span>
        </button>
        <span class="filters-total">
          {{ t('uninstaller.summaryCount', { count: store.filterCounts.all }, store.filterCounts.all) }}
          · {{ t('uninstaller.summarySize') }} <strong>{{ formatBytes(store.totalBytes) }}</strong>
        </span>
      </nav>

      <p v-if="store.storeUnavailable" class="notice">{{ t('uninstaller.storeUnavailable') }}</p>

      <section v-if="store.removedElsewhere.length" class="removed">
        <header>
          <History :size="16" :stroke-width="1.9" aria-hidden="true" />
          <div>
            <strong>{{ t('uninstaller.removed.title', { count: store.removedElsewhere.length }, store.removedElsewhere.length) }}</strong>
            <span>{{ t('uninstaller.removed.body') }}</span>
          </div>
        </header>
        <ul>
          <li v-for="item in store.removedElsewhere.slice(0, 5)" :key="item.id">
            <PtProgramIcon :name="item.name" :size="26" />
            <span class="removed-copy">
              <strong>{{ item.name }}</strong>
              <small>{{ [item.publisher, item.version].filter(Boolean).join(' · ') }}</small>
            </span>
            <button type="button" class="text-btn muted" :disabled="store.busy" @click="store.dismissRemoved(item.id)">
              {{ t('uninstaller.removed.dismiss') }}
            </button>
            <button type="button" class="row-btn" :disabled="store.busy" @click="onForce(item.id)">
              {{ t('uninstaller.removed.scan') }}
            </button>
          </li>
        </ul>
      </section>

      <div class="list-card">
        <div class="list-head">
          <label class="col-check">
            <input
              type="checkbox"
              :checked="visibleAllSelected"
              :aria-label="t('uninstaller.selectAllVisible')"
              @change="store.setVisibleSelected(!visibleAllSelected)"
            />
          </label>
          <span class="col-name">{{ t('uninstaller.columns.name') }}</span>
          <span class="col-size">{{ t('uninstaller.columns.size') }}</span>
          <span class="col-date">{{ t('uninstaller.columns.installed') }}</span>
          <span class="col-action" />
        </div>

        <ul v-if="store.visiblePrograms.length" class="rows">
          <li
            v-for="program in store.visiblePrograms"
            :key="program.id"
            class="row"
            :class="{ selected: store.selectedIdSet.has(program.id) }"
          >
            <label class="col-check">
              <input
                type="checkbox"
                :checked="store.selectedIdSet.has(program.id)"
                :aria-label="program.name"
                @change="store.toggleSelected(program.id)"
              />
            </label>
            <div class="col-name name-cell">
              <PtProgramIcon
                :name="program.name"
                :src="store.icons[program.id]"
                :plated="program.source === 'store'"
                :size="32"
              />
              <span class="name-copy">
                <span class="name-line">
                  <strong :title="program.name">{{ program.name }}</strong>
                  <span v-if="program.source === 'store'" class="badge">{{ t('uninstaller.badges.store') }}</span>
                  <span v-if="program.isRuntime" class="badge">{{ t('uninstaller.badges.runtime') }}</span>
                  <span
                    v-if="program.machineWide && !store.isAdmin"
                    class="badge badge-admin"
                    :title="t('uninstaller.badgeHints.admin')"
                  >{{ t('uninstaller.badges.admin') }}</span>
                </span>
                <small>{{ [program.publisher, program.version].filter(Boolean).join(' · ') }}</small>
              </span>
            </div>
            <span class="col-size">{{
              program.sizeBytes ? formatBytes(program.sizeBytes) : t('uninstaller.unknownSize')
            }}</span>
            <span class="col-date">{{ formatDate(program.installDate) }}</span>
            <span class="col-action">
              <button
                type="button"
                class="row-btn"
                :disabled="store.busy"
                @click="openConfirm([program])"
              >
                <Trash2 :size="14" :stroke-width="1.9" aria-hidden="true" />
                {{ t('uninstaller.uninstall') }}
              </button>
            </span>
          </li>
        </ul>

        <PtEmptyState
          v-else-if="store.loaded"
          :title="t('uninstaller.emptyTitle')"
          :description="t('uninstaller.emptyBody')"
        />
      </div>

      <footer v-if="store.selectedIds.length" class="action-bar">
        <div class="action-copy">
          <strong>{{ t('uninstaller.selectedSummary', { count: store.selectedIds.length }, store.selectedIds.length) }}</strong>
          <span v-if="store.selectedBytes">
            {{ t('uninstaller.selectedSize') }} <em>{{ formatBytes(store.selectedBytes) }}</em>
          </span>
          <button type="button" class="text-btn" @click="store.clearSelection()">
            {{ t('uninstaller.clearSelection') }}
          </button>
        </div>
        <button
          type="button"
          class="pt-btn pt-btn-primary"
          :disabled="store.busy"
          @click="openConfirm(store.selectedPrograms)"
        >
          {{ t('uninstaller.uninstallSelected') }}
        </button>
      </footer>
    </div>

    <PtUninstallProgressDialog
      v-if="showDialog"
      :operation="store.operation"
      :progress="store.progress"
      :program="store.currentProgram"
      :icon-src="store.currentProgram ? (store.icons[store.currentProgram.id] ?? null) : null"
      :queue-index="store.queueIndex"
      :queue-total="store.queue.length"
      :restore-point-requested="store.restorePointRequested"
      :forced="store.forced"
      :areas-done="store.areasDone"
      :clean-log="store.cleanLog"
      :clean-total="store.cleanTotal"
      :clean-processed="store.cleanProcessed"
      :clean-issues="store.cleanIssues"
      @cancel="store.cancel()"
      @focus-uninstaller="store.focusUninstaller()"
    />

    <PtUninstallConfirm
      v-model:open="confirmOpen"
      :programs="confirmTargets"
      :icons="store.icons"
      :is-admin="store.isAdmin"
      @confirm="onConfirm"
      @force="onForce"
      @restart-admin="store.restartAsAdministrator()"
    />
  </PtPageShell>
</template>

<style scoped>
.listing-feed {
  display: flex;
  flex-direction: column;
  gap: 2px;
  height: 172px;
  margin: 12px 0 0;
  padding: 6px;
  overflow: hidden;
  list-style: none;
  border-radius: 12px;
  background: var(--surface-soft);
}
.listing-feed li {
  display: flex;
  min-width: 0;
  align-items: center;
  gap: 10px;
  min-height: 26px;
  padding: 0 6px;
  font-size: 0.8125rem;
  transition: opacity 260ms ease;
}
.feed-name {
  min-width: 0;
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.listing-feed li:first-child {
  font-weight: 650;
}
.feed-ok {
  flex: none;
  color: var(--success);
}
.feed-dot {
  width: 8px;
  height: 8px;
  flex: none;
  border-radius: 50%;
  background: var(--primary);
  animation: feed-pulse 0.9s ease-in-out infinite;
}
.feed-enter-active {
  transition:
    opacity 240ms ease,
    transform 240ms cubic-bezier(0.22, 1, 0.36, 1);
}
.feed-enter-from {
  opacity: 0;
  transform: translateY(-8px);
}
.feed-move {
  transition: transform 240ms cubic-bezier(0.22, 1, 0.36, 1);
}
.feed-leave-active {
  display: none;
}
@keyframes feed-pulse {
  50% {
    opacity: 0.35;
  }
}
@media (prefers-reduced-motion: reduce) {
  .feed-enter-active,
  .feed-move {
    transition: none;
  }
}
.programs {
  display: flex;
  flex: 1;
  min-height: 0;
  flex-direction: column;
  gap: 10px;
}
.toolbar {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 10px;
}
.search {
  display: flex;
  min-width: 240px;
  flex: 1;
  align-items: center;
  gap: 8px;
  height: 36px;
  padding: 0 12px;
  border: 1px solid color-mix(in oklab, var(--border) 80%, transparent);
  border-radius: 10px;
  background: var(--card);
  color: var(--muted-foreground);
}
.search:focus-within {
  border-color: color-mix(in oklab, var(--primary) 50%, var(--border));
}
.search input {
  min-width: 0;
  flex: 1;
  border: 0;
  outline: 0;
  background: transparent;
  color: var(--foreground);
  font-size: 0.8125rem;
}
.sort {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  min-width: 190px;
  color: var(--muted-foreground);
  font-size: 0.75rem;
  font-weight: 600;
}
.runtimes {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  color: var(--muted-foreground);
  font-size: 0.75rem;
  font-weight: 600;
  cursor: pointer;
}
.filters {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 6px;
}
.chip {
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
.chip:hover {
  color: var(--foreground);
  background: color-mix(in oklab, var(--muted) 45%, var(--card));
}
.chip.active {
  border-color: color-mix(in oklab, var(--primary) 40%, var(--border));
  background: color-mix(in oklab, var(--primary) 10%, var(--card));
  color: var(--foreground);
}
.chip-count {
  min-width: 18px;
  padding: 0 5px;
  border-radius: 999px;
  background: var(--muted);
  font-size: 0.625rem;
  line-height: 16px;
  text-align: center;
}
.filters-total {
  margin-left: auto;
  color: var(--muted-foreground);
  font-size: 0.75rem;
}
.filters-total strong {
  color: var(--foreground);
}
.notice {
  margin: 0;
  padding: 8px 10px;
  border-radius: 10px;
  background: color-mix(in oklab, var(--warning) 12%, var(--card));
  font-size: 0.75rem;
}
.removed {
  padding: 10px 12px;
  border: 1px solid color-mix(in oklab, var(--primary) 30%, var(--border));
  border-radius: 14px;
  background: color-mix(in oklab, var(--primary) 6%, var(--card));
}
.removed header {
  display: flex;
  align-items: flex-start;
  gap: 10px;
}
.removed header svg {
  flex: none;
  margin-top: 2px;
  color: var(--primary);
}
.removed header strong {
  display: block;
  font-size: 0.8125rem;
}
.removed header span {
  color: var(--muted-foreground);
  font-size: 0.75rem;
}
.removed ul {
  display: flex;
  flex-direction: column;
  gap: 2px;
  list-style: none;
  margin: 8px 0 0;
  padding: 0;
}
.removed li {
  display: flex;
  align-items: center;
  gap: 10px;
  min-height: 40px;
  padding: 4px 6px;
  border-radius: 10px;
}
.removed li:hover {
  background: color-mix(in oklab, var(--muted) 40%, transparent);
}
.removed-copy {
  display: flex;
  min-width: 0;
  flex: 1;
  flex-direction: column;
}
.removed-copy strong {
  overflow: hidden;
  font-size: 0.8125rem;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.removed-copy small {
  color: var(--muted-foreground);
  font-size: 0.6875rem;
}
.text-btn.muted {
  color: var(--muted-foreground);
}
.list-card {
  display: flex;
  min-height: 0;
  flex: 1;
  flex-direction: column;
  overflow: hidden;
  border: 1px solid color-mix(in oklab, var(--border) 75%, transparent);
  border-radius: 14px;
  background: var(--card);
}
.list-head,
.row {
  display: grid;
  grid-template-columns: 34px minmax(0, 1fr) 104px 132px 118px;
  align-items: center;
  gap: 0;
  padding: 0 12px;
}
.list-head {
  /* Rows sit 4px in from each side (margin); match it so columns line up with their headings. */
  padding: 0 16px;
  /* Reserve the same scrollbar width as the row list below, or every column shifts. */
  overflow-y: hidden;
  scrollbar-gutter: stable;
  height: 36px;
  border-bottom: 1px solid color-mix(in oklab, var(--border) 65%, transparent);
  color: var(--muted-foreground);
  font-size: 0.6875rem;
  font-weight: 700;
  letter-spacing: 0.04em;
  text-transform: uppercase;
}
.rows {
  min-height: 0;
  flex: 1;
  overflow: auto;
  list-style: none;
  margin: 0;
  padding: 4px 0;
  scrollbar-gutter: stable;
}
.row {
  min-height: 52px;
  margin: 0 4px;
  border-radius: 12px;
}
.row:hover {
  background: color-mix(in oklab, var(--muted) 40%, transparent);
}
.row.selected {
  background: color-mix(in oklab, var(--primary) 8%, transparent);
}
.col-check {
  display: grid;
  place-items: center;
  cursor: pointer;
}
.col-size,
.col-date {
  height: 100%;
  display: flex;
  align-items: center;
  color: var(--muted-foreground);
  font-size: 0.75rem;
  font-variant-numeric: tabular-nums;
  white-space: nowrap;
}
/* Size: right-aligned numbers. Installed: left-aligned dates after a fixed gap. */
.col-size {
  justify-content: flex-end;
  padding-right: 18px;
}
.col-date {
  padding-left: 18px;
}
.col-name {
  padding-right: 12px;
}

.row .col-size {
  font-weight: 650;
}
.col-action {
  display: flex;
  justify-content: flex-end;
}
.name-cell {
  display: flex;
  min-width: 0;
  align-items: center;
  gap: 10px;
}
.name-copy {
  display: flex;
  min-width: 0;
  flex-direction: column;
  gap: 1px;
}
.name-line {
  display: flex;
  min-width: 0;
  align-items: center;
  gap: 6px;
}
.name-line strong {
  overflow: hidden;
  font-size: 0.8125rem;
  font-weight: 650;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.name-copy small {
  overflow: hidden;
  color: var(--muted-foreground);
  font-size: 0.6875rem;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.badge {
  display: inline-flex;
  flex: none;
  padding: 1px 7px;
  border-radius: 999px;
  background: color-mix(in oklab, var(--primary) 12%, transparent);
  color: var(--primary);
  font-size: 0.625rem;
  font-weight: 700;
}
.badge-admin {
  background: var(--muted);
  color: var(--muted-foreground);
}
.row-btn {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  height: 30px;
  padding: 0 10px;
  border: 1px solid color-mix(in oklab, var(--border) 80%, transparent);
  border-radius: 9px;
  background: var(--card);
  color: var(--foreground);
  font-size: 0.75rem;
  font-weight: 600;
  cursor: pointer;
}
.row-btn:hover:not(:disabled) {
  border-color: color-mix(in oklab, var(--destructive) 45%, var(--border));
  background: color-mix(in oklab, var(--destructive) 8%, var(--card));
  color: var(--destructive);
}
.row-btn:disabled {
  cursor: default;
  opacity: 0.55;
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
  align-items: baseline;
  gap: 4px 12px;
}
.action-copy strong {
  font-size: 0.8125rem;
}
.action-copy span {
  color: var(--muted-foreground);
  font-size: 0.75rem;
}
.action-copy em {
  color: var(--foreground);
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
@media (max-width: 900px) {
  .list-head,
  .row {
    grid-template-columns: 34px minmax(0, 1fr) 84px 110px;
  }
  .col-date {
    display: none;
  }
}
</style>

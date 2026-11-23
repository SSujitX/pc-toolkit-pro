<script setup lang="ts">
import { computed, onUnmounted, ref, watch } from 'vue';
import { useI18n } from 'vue-i18n';
import {
  Bot,
  Boxes,
  FolderGit2,
  FolderOpen,
  Globe,
  Layers,
  Package,
  Paintbrush,
  Sparkles,
} from '@lucide/vue';
import type { Component } from 'vue';
import PtPageShell from '@/components/custom/pt-page-shell.vue';
import PtOperationWorkspace from '@/components/custom/pt-operation-workspace.vue';
import PtSoftSelect from '@/components/custom/pt-soft-select.vue';
import PtDeepCleanupConfirm from '@/pages/deep-cleaner/pt-deep-cleanup-confirm.vue';
import PtDeepCleanupResult from '@/pages/deep-cleaner/pt-deep-cleanup-result.vue';
import { formatBytes } from '@/lib/utils/format';
import {
  execReasonKey,
  groupBytes,
  groupCount,
  ruleDetail,
  ruleTitle,
  tagLabelKey,
  type DeepCleanupGroup,
  type DeepCleanupRule,
  type DeepCleanupRuleExecResult,
  type DeepCleanupSelectionMode,
} from '@/lib/models/deep-cleaner';
import { useDeepCleanerStore } from '@/stores/deep-cleaner-store';

const { t, locale } = useI18n();
const store = useDeepCleanerStore();
const confirmOpen = ref(false);
const nowMs = ref(Date.now());
const activeGroup = ref<DeepCleanupGroup>('system');
let elapsedTimer: number | null = null;

const groupIcons: Record<DeepCleanupGroup, Component> = {
  system: Layers,
  application: Package,
  browser: Globe,
  development: Boxes,
  projects: FolderGit2,
  ai: Bot,
};

const isBusy = computed(() => store.loading);
const showResults = computed(
  () => store.hasScanned && !isBusy.value && (store.visibleRules.length > 0 || store.rules.length > 0)
);
const selectedCount = computed(() => store.selectedRules.length);
const selectedBytes = computed(() => store.selectedBytes);
const foundCount = computed(() => store.visibleRules.length);
const foundBytes = computed(() => store.foundBytes);

// Groups come from the scan (data-driven); a group shows only when it has findings.
const groupNav = computed(() =>
  store.groups.map((group) => ({
    id: group,
    label: t(`deepCleaner.groups.${group}`),
    count: groupCount(store.rules, group),
    bytes: groupBytes(store.rules, group),
    icon: groupIcons[group],
  })).filter((g) => g.count > 0)
);

const activeRules = computed(() =>
  store.visibleRules.filter((r) => r.group === activeGroup.value)
);

// Project discovery hit a safety limit: say so above the list.
const activeGroupLimited = computed(() =>
  activeRules.value.some((r) => r.reason === 'discoveryLimited')
);

const activeGroupMeta = computed(
  () => groupNav.value.find((g) => g.id === activeGroup.value) ?? groupNav.value[0] ?? null
);

const groupAllSelected = computed(
  () => activeRules.value.length > 0 && activeRules.value.every((r) => r.selected)
);

const progressPct = computed(() => {
  if (!isBusy.value) return 0;
  const p = store.progress;
  if (p?.total && p.total > 0) {
    return Math.min(100, Math.round((Math.max(0, p.current ?? 0) / p.total) * 100));
  }
  if (p?.phase === 'executing') {
    const total = Math.max(1, selectedCount.value);
    return Math.min(100, Math.round((p.itemsScanned / total) * 100));
  }
  return 4;
});

const isCleaning = computed(() => store.progress?.phase === 'executing');
const statusLabel = computed(() =>
  isCleaning.value ? t('deepCleaner.cleaning') : t('deepCleaner.analyzing')
);
const statusTitle = computed(() =>
  isCleaning.value
    ? t('deepCleaner.cleaningTitle')
    : t('deepCleaner.analyzingTitle')
);
const sourceValue = computed(() => {
  const path = store.progress?.currentPath;
  if (path && path.includes('\\')) return path;
  if (store.progress?.message?.startsWith('deepCleaner.')) {
    return t(store.progress.message);
  }
  return path || t('deepCleaner.preparing');
});

const stats = computed(() => {
  if (isCleaning.value) {
    return [
      { label: t('deepCleaner.selected'), value: String(selectedCount.value) },
      { label: t('deepCleaner.estimated'), value: formatBytes(selectedBytes.value) },
      { label: t('deepCleaner.elapsed'), value: t('common.elapsedSeconds', { count: elapsedSec.value }) },
    ];
  }
  return [
    {
      label: t('deepCleaner.filesChecked'),
      value: String(store.progress?.itemsScanned ?? 0),
    },
    {
      label: t('deepCleaner.dataScanned'),
      value: formatBytes(store.progress?.bytesScanned ?? 0),
    },
    { label: t('deepCleaner.elapsed'), value: t('common.elapsedSeconds', { count: elapsedSec.value }) },
  ];
});

const selectionOptions = computed(() => [
  { value: 'smart', label: t('deepCleaner.selection.smart') },
  { value: 'all', label: t('deepCleaner.selection.all') },
  { value: 'none', label: t('deepCleaner.selection.none') },
  // Shown (not chosen) once the user ticks items by hand.
  ...(store.selectionMode === 'manual' ? [{ value: 'manual', label: t('deepCleaner.selection.manual') }] : []),
]);

// Elapsed derives from the store's start timestamp, so it stays right after re-mounting mid-run.
const elapsedSec = computed(() =>
  store.busySince == null ? 0 : Math.max(0, Math.floor((nowMs.value - store.busySince) / 1000))
);
watch(
  isBusy,
  (busy) => {
    if (elapsedTimer != null) window.clearInterval(elapsedTimer);
    elapsedTimer = null;
    if (!busy) return;
    nowMs.value = Date.now();
    elapsedTimer = window.setInterval(() => {
      nowMs.value = Date.now();
    }, 1000);
  },
  { immediate: true }
);

// Immediate: after navigating back the page re-mounts with 'system', which may have no findings.
watch(
  () => store.visibleRules,
  (rules) => {
    if (!rules.length) return;
    if (!rules.some((r) => r.group === activeGroup.value)) {
      activeGroup.value = rules[0]?.group ?? 'system';
    }
  },
  { immediate: true }
);

onUnmounted(() => {
  if (elapsedTimer != null) window.clearInterval(elapsedTimer);
});

function toggleGroupSelectAll() {
  store.setGroupSelected(activeGroup.value, !groupAllSelected.value);
}

function onSelectionMode(mode: string | number) {
  store.applySelectionMode(String(mode) as DeepCleanupSelectionMode);
}

function riskLabel(risk: string) {
  return risk === 'safe' ? t('deepCleaner.riskLow') : t('deepCleaner.riskMedium');
}

/** Display name of the first running related app (`chrome.exe` → `chrome`). */
function appLabel(rule: DeepCleanupRule): string {
  const image = rule.runningProcesses[0] ?? rule.relatedProcesses[0] ?? '';
  return image.replace(/\.exe$/i, '');
}

function execResult(rule: DeepCleanupRule): DeepCleanupRuleExecResult | undefined {
  return store.resultById[rule.id];
}

function execLabel(item: DeepCleanupRuleExecResult): string {
  const status = t(`deepCleaner.execStatus.${item.status}`);
  if (!item.reason) return status;
  if (item.reason === 'itemsBusy') {
    return `${status} · ${t('deepCleaner.execReason.itemsBusy', { count: item.busyCount })}`;
  }
  if (item.reason === 'itemsFailed') {
    return `${status} · ${t('deepCleaner.execReason.itemsFailed', { count: item.failedCount })}`;
  }
  return `${status} · ${t(execReasonKey(item.reason))}`;
}

/** Item-level rows (projects, AI models, toolchains) get a second line. */
function hasItemFacts(rule: DeepCleanupRule): boolean {
  return Boolean(rule.displayName || rule.artifacts?.length || rule.tags?.length);
}

function modifiedLabel(rule: DeepCleanupRule): string {
  if (!rule.lastModifiedMs) return '';
  const date = new Intl.DateTimeFormat(locale.value, { dateStyle: 'medium' }).format(
    new Date(rule.lastModifiedMs)
  );
  return t('deepCleaner.projects.lastModified', { date });
}

function sizeLabel(rule: DeepCleanupRule): string {
  return rule.status === 'unmeasured' ? t('deepCleaner.special.sizeUnknown') : formatBytes(rule.bytes);
}

function reasonHint(rule: DeepCleanupRule): string {
  return rule.reason && rule.reason !== 'discoveryLimited'
    ? t(`deepCleaner.special.reason.${rule.reason}`)
    : '';
}
</script>

<template>
  <PtPageShell
    :title="t('deepCleaner.title')"
    :subtitle="t('deepCleaner.subtitle')"
    content-mode="workspace"
  >
    <template #actions>
      <button
        v-if="showResults"
        type="button"
        class="pt-btn"
        :disabled="isBusy"
        @click="store.scan()"
      >
        {{ t('deepCleaner.scanAgain') }}
      </button>
      <button
        v-else-if="!isBusy"
        type="button"
        class="pt-btn pt-btn-primary"
        @click="store.scan()"
      >
        {{ t('deepCleaner.startScan') }}
      </button>
    </template>

    <PtOperationWorkspace
      v-if="isBusy"
      :status="statusLabel"
      :title="statusTitle"
      :source-label="t('deepCleaner.scanning')"
      :source-value="sourceValue"
      :source-icon="FolderOpen"
      :progress="progressPct"
      :stats="stats"
      :hint="isCleaning ? t('deepCleaner.cleanHint') : t('deepCleaner.scanHint')"
      :icon="Paintbrush"
      :cancel-label="t('common.cancel')"
      @cancel="store.cancel()"
    />

    <div v-else-if="showResults && foundCount > 0" class="results">
      <header class="summary">
        <div class="summary-copy">
          <strong>{{ t('deepCleaner.summaryCount', { count: foundCount }, foundCount) }}</strong>
          <span class="summary-space">
            {{ t('deepCleaner.summarySpace') }}
            <em>{{ formatBytes(foundBytes) }}</em>
          </span>
        </div>
        <label class="mode">
          <Sparkles :size="14" :stroke-width="2" aria-hidden="true" />
          <span class="mode-label">{{ t('deepCleaner.selectionLabel') }}</span>
          <PtSoftSelect
            :model-value="store.selectionMode"
            :options="selectionOptions"
            @update:model-value="onSelectionMode"
          />
        </label>
      </header>

      <div class="browser">
        <aside class="nav">
          <button
            v-for="group in groupNav"
            :key="group.id"
            type="button"
            class="nav-item"
            :class="{ active: activeGroup === group.id }"
            @click="activeGroup = group.id"
          >
            <span class="nav-icon" aria-hidden="true">
              <component :is="group.icon" :size="16" :stroke-width="1.75" />
            </span>
            <span class="nav-copy">
              <strong>{{ group.label }}</strong>
              <small>
                {{ formatBytes(group.bytes) }} ·
                {{ t('deepCleaner.categoryItems', { count: group.count }, group.count) }}
              </small>
            </span>
            <em>{{ formatBytes(group.bytes) }}</em>
          </button>
        </aside>

        <section class="detail">
          <header class="detail-head">
            <div>
              <h2>{{ activeGroupMeta?.label }}</h2>
              <p>
                {{ t('deepCleaner.selectedOf', {
                  selected: formatBytes(
                    activeRules.filter((r) => r.selected).reduce((n, r) => n + r.bytes, 0)
                  ),
                  total: formatBytes(activeGroupMeta?.bytes ?? 0),
                }) }}
              </p>
            </div>
            <button type="button" class="text-btn" @click="toggleGroupSelectAll">
              {{ groupAllSelected ? t('deepCleaner.deselectAll') : t('deepCleaner.selectAll') }}
            </button>
          </header>
          <p v-if="activeGroupLimited" class="group-notice">
            {{ t('deepCleaner.projects.limitedNotice') }}
          </p>

          <ul class="rule-list">
            <li v-for="rule in activeRules" :key="rule.id" class="rule-row">
              <label class="rule-check">
                <input
                  type="checkbox"
                  :checked="rule.selected"
                  @change="store.setRuleSelected(rule.id, !rule.selected)"
                />
                <span class="rule-icon" aria-hidden="true">
                  <component
                    :is="groupIcons[rule.group]"
                    :size="15"
                    :stroke-width="1.75"
                  />
                </span>
                <span class="rule-body">
                <span class="rule-copy">
                  <strong :title="rule.pathHint">{{ ruleTitle(rule, t) }}</strong>
                  <span
                    v-if="rule.active"
                    class="badge badge-admin"
                    :title="t('deepCleaner.projects.activeHint')"
                  >{{ t('deepCleaner.projects.active') }}</span>
                  <span v-if="rule.risk === 'safe'" class="badge">{{ riskLabel(rule.risk) }}</span>
                  <span
                    v-if="rule.needsAdmin"
                    class="badge badge-admin"
                    :title="t('deepCleaner.adminRequiredHint')"
                  >{{ t('deepCleaner.status.needsAdmin') }}</span>
                  <span
                    v-else-if="rule.requiresElevation"
                    class="badge badge-muted"
                    :title="t('deepCleaner.adminRequiredHint')"
                  >{{ t('deepCleaner.adminRequired') }}</span>
                  <span
                    v-if="rule.status === 'requiresClose'"
                    class="badge badge-admin"
                    :title="t('deepCleaner.status.closeAppHint', { app: appLabel(rule) })"
                  >{{ t('deepCleaner.status.closeApp', { app: appLabel(rule) }) }}</span>
                  <span
                    v-if="rule.limited"
                    class="badge badge-muted"
                    :title="t('deepCleaner.status.limitedHint')"
                  >{{ t('deepCleaner.status.limited') }}</span>
                </span>
                <span v-if="hasItemFacts(rule)" class="rule-sub">
                  <span
                    v-for="tag in rule.tags ?? []"
                    :key="tag"
                    class="chip"
                  >{{ t(tagLabelKey(rule, tag)) }}</span>
                  <span class="rule-sub-text">{{ ruleDetail(rule, t) }}</span>
                  <span v-if="modifiedLabel(rule)" class="rule-sub-text">· {{ modifiedLabel(rule) }}</span>
                </span>
                <span v-else-if="reasonHint(rule)" class="rule-sub">
                  <span class="rule-sub-text">{{ reasonHint(rule) }}</span>
                </span>
                </span>
              </label>
              <div class="rule-meta">
                <strong :title="reasonHint(rule) || undefined">{{ sizeLabel(rule) }}</strong>
                <small
                  v-if="execResult(rule)"
                  class="exec-label"
                  :class="execResult(rule)?.status"
                >{{ execLabel(execResult(rule)!) }}</small>
                <small v-else>{{
                  rule.selected ? t('deepCleaner.selectedLabel') : t('deepCleaner.cleanableLabel')
                }}</small>
              </div>
            </li>
          </ul>
        </section>
      </div>

      <footer class="action-bar">
        <div class="action-copy">
          <strong>
            {{ t('deepCleaner.footerSelected', { count: selectedCount }, selectedCount) }}
          </strong>
          <span>
            {{ t('deepCleaner.footerEstimate') }}
            <em>{{ formatBytes(selectedBytes) }}</em>
          </span>
        </div>
        <button
          type="button"
          class="pt-btn pt-btn-primary"
          :disabled="selectedCount === 0"
          @click="confirmOpen = true"
        >
          {{ t('deepCleaner.clean') }}
        </button>
      </footer>

      <PtDeepCleanupResult
        v-if="store.result"
        :result="store.result"
        :rules="store.rules"
        @dismiss="store.dismissResult()"
      />
    </div>

    <div v-else-if="showResults && foundCount === 0" class="empty">
      <p>{{ t('deepCleaner.emptyClean') }}</p>
      <button type="button" class="pt-btn pt-btn-primary" @click="store.scan()">
        {{ t('deepCleaner.scanAgain') }}
      </button>
    </div>

    <div v-else class="hero">
      <div class="hero-card">
        <span class="hero-icon" aria-hidden="true">
          <Layers :size="22" :stroke-width="1.75" />
        </span>
        <h2>{{ t('deepCleaner.findTitle') }}</h2>
        <p>{{ t('deepCleaner.findBody') }}</p>
        <button type="button" class="pt-btn pt-btn-primary" @click="store.scan()">
          {{ t('deepCleaner.startScan') }}
        </button>
      </div>
    </div>

    <PtDeepCleanupConfirm
      v-model:open="confirmOpen"
      :rules="store.selectedRules"
      :selected-bytes="selectedBytes"
      @confirm="store.execute()"
    />
  </PtPageShell>
</template>

<style scoped>
.hero {
  display: grid;
  flex: 1;
  width: 100%;
  min-height: 0;
  place-items: center;
  place-content: center;
  padding: 24px;
}
.hero-card {
  width: min(440px, 100%);
  text-align: center;
  border: 1px solid color-mix(in oklab, var(--border) 75%, transparent);
  border-radius: 16px;
  background: var(--card);
  box-shadow: var(--shadow-card);
  padding: 28px 24px;
}
.hero-icon {
  display: grid;
  width: 48px;
  height: 48px;
  margin: 0 auto 14px;
  place-items: center;
  border-radius: 14px;
  background: color-mix(in oklab, var(--primary) 12%, transparent);
  color: var(--primary);
}
.hero-card h2 {
  margin: 0;
  font-size: 1.125rem;
  font-weight: 700;
}
.hero-card p {
  margin: 8px 0 18px;
  color: var(--muted-foreground);
  font-size: 0.8125rem;
  line-height: 1.45;
}
.results {
  display: flex;
  flex: 1;
  min-height: 0;
  flex-direction: column;
  gap: 12px;
}
.summary {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  padding: 4px 2px;
}
.summary-copy {
  display: flex;
  flex-wrap: wrap;
  align-items: baseline;
  gap: 10px;
  min-width: 0;
}
.summary strong {
  font-size: 0.9375rem;
}
.summary-space {
  color: var(--muted-foreground);
  font-size: 0.8125rem;
}
.summary em {
  font-style: normal;
  font-weight: 700;
  color: var(--primary);
  font-size: 1.125rem;
}
.mode-label {
  color: var(--muted-foreground);
  font-size: 0.75rem;
  font-weight: 600;
}
.browser {
  display: grid;
  flex: 1;
  min-height: 0;
  grid-template-columns: 240px minmax(0, 1fr);
  gap: 0;
  border: 1px solid color-mix(in oklab, var(--border) 75%, transparent);
  border-radius: 14px;
  background: var(--card);
  overflow: hidden;
}
.nav {
  display: flex;
  flex-direction: column;
  gap: 4px;
  padding: 10px;
  border-right: 1px solid color-mix(in oklab, var(--border) 70%, transparent);
  background: var(--surface-soft);
  overflow: auto;
}
.nav-item {
  display: flex;
  align-items: center;
  gap: 10px;
  width: 100%;
  padding: 10px;
  border: 1px solid transparent;
  border-radius: 12px;
  background: transparent;
  color: var(--foreground);
  text-align: left;
  cursor: pointer;
}
.nav-item:hover {
  background: color-mix(in oklab, var(--muted) 55%, transparent);
}
.nav-item.active {
  border-color: color-mix(in oklab, var(--primary) 35%, var(--border));
  background: color-mix(in oklab, var(--primary) 10%, var(--card));
}
.nav-icon {
  display: grid;
  width: 30px;
  height: 30px;
  flex: none;
  place-items: center;
  border-radius: 9px;
  background: color-mix(in oklab, var(--primary) 12%, transparent);
  color: var(--primary);
}
.nav-copy {
  min-width: 0;
  flex: 1;
}
.nav-copy strong {
  display: block;
  font-size: 0.8125rem;
}
.nav-copy small {
  display: block;
  margin-top: 2px;
  color: var(--muted-foreground);
  font-size: 0.6875rem;
}
.nav-item > em {
  font-style: normal;
  font-size: 0.75rem;
  font-weight: 650;
  color: var(--muted-foreground);
}
.detail {
  display: flex;
  min-width: 0;
  flex-direction: column;
  overflow: auto;
}
.detail-head {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 12px;
  padding: 14px 16px 10px;
  border-bottom: 1px solid color-mix(in oklab, var(--border) 65%, transparent);
}
.detail-head h2 {
  margin: 0;
  font-size: 0.9375rem;
}
.detail-head p {
  margin: 4px 0 0;
  color: var(--muted-foreground);
  font-size: 0.75rem;
}
.text-btn {
  border: 0;
  background: transparent;
  color: var(--primary);
  font-size: 0.75rem;
  font-weight: 650;
  cursor: pointer;
}
.rule-list {
  list-style: none;
  margin: 0;
  padding: 6px 8px 14px;
}
.rule-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  min-height: 48px;
  padding: 8px 10px;
  border-radius: 12px;
}
.rule-row:hover {
  background: color-mix(in oklab, var(--muted) 40%, transparent);
}
.rule-check {
  display: flex;
  align-items: center;
  gap: 10px;
  min-width: 0;
  flex: 1;
  cursor: pointer;
}
.rule-check input {
  flex: none;
}
.rule-icon {
  display: grid;
  width: 28px;
  height: 28px;
  flex: none;
  place-items: center;
  border-radius: 8px;
  background: color-mix(in oklab, var(--primary) 10%, transparent);
  color: var(--primary);
}
.rule-body {
  display: flex;
  min-width: 0;
  flex-direction: column;
  gap: 3px;
}
.rule-copy {
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
}
.rule-copy strong {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.rule-sub {
  display: flex;
  align-items: center;
  gap: 6px;
  min-width: 0;
  color: var(--muted-foreground);
  font-size: 0.6875rem;
}
.rule-sub-text {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.chip {
  display: inline-flex;
  flex: none;
  padding: 1px 6px;
  border: 1px solid color-mix(in oklab, var(--border) 80%, transparent);
  border-radius: 999px;
  background: color-mix(in oklab, var(--muted) 55%, transparent);
  color: var(--foreground);
  font-size: 0.625rem;
  font-weight: 600;
}
.group-notice {
  margin: 0 10px 6px;
  padding: 6px 10px;
  border-radius: 10px;
  background: color-mix(in oklab, var(--muted) 60%, transparent);
  color: var(--muted-foreground);
  font-size: 0.75rem;
}
.rule-copy strong {
  font-size: 0.8125rem;
  font-weight: 600;
}
.badge {
  display: inline-flex;
  padding: 2px 7px;
  border-radius: 999px;
  background: color-mix(in oklab, var(--success, #22c55e) 16%, transparent);
  color: color-mix(in oklab, var(--success, #15803d) 80%, var(--foreground));
  font-size: 0.625rem;
  font-weight: 700;
}
.badge-admin {
  background: color-mix(in oklab, var(--warning, #d97706) 18%, transparent);
  color: color-mix(in oklab, var(--warning, #b45309) 85%, var(--foreground));
}
.rule-meta {
  text-align: right;
}
.rule-meta strong {
  display: block;
  font-size: 0.8125rem;
}
.rule-meta small {
  color: var(--muted-foreground);
  font-size: 0.625rem;
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
.action-copy strong {
  display: block;
  font-size: 0.8125rem;
}
.action-copy span {
  color: var(--muted-foreground);
  font-size: 0.75rem;
}
.action-copy em {
  font-style: normal;
  font-weight: 700;
  color: var(--foreground);
}
.mode {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  color: var(--muted-foreground);
  min-width: 220px;
}
.badge-muted {
  background: color-mix(in oklab, var(--muted) 80%, transparent);
  color: var(--muted-foreground);
}
.exec-label {
  color: color-mix(in oklab, var(--warning, #b45309) 85%, var(--foreground));
}
.exec-label.completed {
  color: color-mix(in oklab, var(--success, #15803d) 80%, var(--foreground));
}
.exec-label.failed {
  color: var(--destructive, #dc2626);
}
.empty {
  display: grid;
  flex: 1;
  width: 100%;
  min-height: 0;
  place-items: center;
  place-content: center;
  gap: 14px;
  color: var(--muted-foreground);
}
@media (max-width: 960px) {
  .browser {
    grid-template-columns: 1fr;
  }
  .nav {
    border-right: 0;
    border-bottom: 1px solid color-mix(in oklab, var(--border) 70%, transparent);
  }
}
</style>

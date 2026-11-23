<script setup lang="ts">
import { computed, ref } from 'vue';
import { useI18n } from 'vue-i18n';
import { Check, ChevronDown, ChevronRight, Copy, Info, Minus, RotateCcw, ShieldAlert, Sparkles, TriangleAlert, Wand2 } from '@lucide/vue';
import PtSoftSelect from '@/components/custom/pt-soft-select.vue';
import type { CareSelectionMode } from '@/lib/models/care';
import PtStatusBadge from '@/components/custom/pt-status-badge.vue';
import { careBytes, careIssueCount, careItemKey, type CareCategoryScan, type CareItem, type CareRisk, isOpenAction } from '@/lib/models/care';
import { formatBytes } from '@/lib/utils/format';
import { useCareStore } from '@/stores/care-store';
import {
  CARE_CATEGORY_ICONS,
  actionLabel,
  categoryName,
  formatElapsed,
  itemDetail,
  itemTitle,
  reasonText,
} from './care-ui';

const emit = defineEmits<{ fix: [] }>();
const { t, te } = useI18n();
const store = useCareStore();

/** Collapsed category ids; categories with issues start expanded. */
const collapsed = ref<Set<string>>(
  new Set(store.scans.filter((c) => c.state !== 'issues' && c.state !== 'attention').map((c) => c.category))
);

const selectedCount = computed(() => store.selectedItems.length);

const selectionOptions = computed(() => [
  { value: 'smart', label: t('care.engine.selection.smart') },
  { value: 'all', label: t('care.engine.selection.all') },
  { value: 'none', label: t('care.engine.selection.none') },
  ...(store.selectionMode === 'manual' ? [{ value: 'manual', label: t('care.engine.selection.manual') }] : []),
]);
function onSelectionMode(value: string | number) {
  store.applySelectionMode(String(value) as CareSelectionMode);
}

function toggle(id: string) {
  const next = new Set(collapsed.value);
  if (next.has(id)) next.delete(id);
  else next.add(id);
  collapsed.value = next;
}

function fixableOf(scan: CareCategoryScan): CareItem[] {
  return scan.items.filter((i) => i.fixable);
}

function categoryChecked(scan: CareCategoryScan): boolean {
  const items = fixableOf(scan);
  return items.length > 0 && items.every((i) => store.selection[careItemKey(i)]);
}

function categoryPartial(scan: CareCategoryScan): boolean {
  const items = fixableOf(scan);
  const on = items.filter((i) => store.selection[careItemKey(i)]).length;
  return on > 0 && on < items.length;
}

function stateLabel(scan: CareCategoryScan): string {
  if (scan.state === 'issues') return t('care.engine.states.issues', { count: careIssueCount(scan).toLocaleString() });
  return t(`care.engine.states.${scan.state}`);
}

function riskTone(risk: CareRisk): 'success' | 'warning' | 'default' {
  if (risk === 'safe') return 'success';
  if (risk === 'review') return 'warning';
  return 'default';
}

/* ---------- Item details (click a row to expand) ---------- */
const expanded = ref<Set<string>>(new Set());
const copiedKey = ref<string | null>(null);

function toggleItem(item: CareItem) {
  const key = careItemKey(item);
  const next = new Set(expanded.value);
  if (next.has(key)) next.delete(key);
  else next.add(key);
  expanded.value = next;
}

/** Params shown as facts, in this order; other params are internal and stay hidden. */
const FACT_KEYS = [
  'currentValue',
  'fixValue',
  'current',
  'available',
  'id',
  'source',
  'publisher',
  'seconds',
  'browser',
  'profile',
  'profileFolder',
  'place',
  'target',
  'drive',
  'days',
] as const;
const VALUE_TOKENS = ['notSet', 'unreadable', 'removePolicy'];
/** Technical values shown in a monospace font; names and versions stay in the body font. */
const MONO_FACTS = new Set(['currentValue', 'fixValue', 'id', 'target', 'profileFolder']);

function itemFacts(item: CareItem): { key: string; label: string; value: string; mono: boolean }[] {
  const params = item.params ?? {};
  const facts: { key: string; label: string; value: string; mono: boolean }[] = [];
  for (const key of FACT_KEYS) {
    const raw = params[key];
    if (raw === undefined || raw === '') continue;
    const tokenKey = `care.engine.details.values.${raw}`;
    const value = VALUE_TOKENS.includes(raw) && te(tokenKey) ? t(tokenKey) : raw;
    facts.push({ key, label: t(`care.engine.details.facts.${key}`), value, mono: MONO_FACTS.has(key) });
  }
  if (params.running === 'true') {
    facts.push({
      key: 'running',
      label: t('care.engine.details.facts.running'),
      value: t('care.engine.details.values.browserOpen', { browser: params.browser ?? '' }),
      mono: false,
    });
  }
  return facts;
}

function fixText(item: CareItem): string {
  if (!item.fixable) return t('care.engine.details.notFixable');
  const key = `care.engine.details.fix.${item.category}`;
  return te(key) ? t(key) : t('care.engine.details.fix.generic');
}

async function copyLocation(item: CareItem) {
  if (!item.location) return;
  try {
    await navigator.clipboard.writeText(item.location);
    copiedKey.value = careItemKey(item);
    window.setTimeout(() => {
      if (copiedKey.value === careItemKey(item)) copiedKey.value = null;
    }, 1500);
  } catch {
    // Clipboard unavailable: the location stays selectable in the panel.
  }
}

function itemDisabled(item: CareItem): boolean {
  return !item.fixable || (item.needsAdmin && !(store.report?.isAdmin ?? false));
}
</script>

<template>
  <div class="care-results">
    <header class="toolbar">
      <div class="totals">
        <strong>
          {{
            store.totalBytes > 0
              ? t('care.engine.results.summary', { count: store.totalIssues.toLocaleString(), size: formatBytes(store.totalBytes) }, store.totalIssues)
              : t('care.engine.results.summaryNoSize', { count: store.totalIssues.toLocaleString() }, store.totalIssues)
          }}
        </strong>
        <span>{{ t('care.engine.results.elapsed', { time: formatElapsed(store.report?.elapsedMs ?? 0) }) }}</span>
      </div>
      <div class="toolbar-actions">
        <button type="button" class="pt-btn pt-btn-ghost" @click="store.scan()">
          <RotateCcw :size="15" :stroke-width="2" />
          {{ t('care.engine.results.rescan') }}
        </button>
        <label class="mode">
          <Sparkles :size="14" :stroke-width="2" aria-hidden="true" />
          <span class="mode-label">{{ t('care.engine.selection.label') }}</span>
          <PtSoftSelect
            :model-value="store.selectionMode"
            :options="selectionOptions"
            :aria-label="t('care.engine.selection.label')"
            @update:model-value="onSelectionMode"
          />
        </label>
      </div>
    </header>

    <p class="mode-hint" :data-mode="store.selectionMode">
      <Sparkles v-if="store.selectionMode === 'smart'" :size="13" :stroke-width="2" />
      <TriangleAlert v-else-if="store.selectionMode === 'all'" :size="13" :stroke-width="2" />
      {{ t(`care.engine.selection.hint.${store.selectionMode}`) }}
    </p>

    <p v-if="store.report?.stopped" class="notice">
      <TriangleAlert :size="14" :stroke-width="2" />
      {{ t('care.engine.results.stopped') }}
    </p>

    <div v-if="store.totalIssues === 0" class="all-clean">
      <span class="all-clean-icon"><Check :size="22" :stroke-width="2.25" /></span>
      <strong>{{ t('care.engine.results.allClean') }}</strong>
      <span>{{ t('care.engine.results.allCleanBody') }}</span>
    </div>

    <div class="sections">
      <section
        v-for="scan in store.scans"
        :key="scan.category"
        class="section"
        :data-state="scan.state"
        :data-open="!collapsed.has(scan.category)"
      >
        <header class="section-head">
          <input
            v-if="fixableOf(scan).length"
            type="checkbox"
            class="check"
            :checked="categoryChecked(scan)"
            :indeterminate="categoryPartial(scan)"
            :aria-label="categoryName(scan.category, t)"
            @change="store.setCategorySelected(scan.category, !categoryChecked(scan))"
          />
          <span v-else class="check-spacer" />
          <button type="button" class="section-toggle" :disabled="!scan.items.length" @click="toggle(scan.category)">
            <span class="section-icon" aria-hidden="true">
              <component :is="CARE_CATEGORY_ICONS[scan.category]" :size="15" :stroke-width="1.75" />
            </span>
            <span class="section-name">{{ categoryName(scan.category, t) }}</span>
            <span class="section-state" :data-state="scan.state">
              <Check v-if="scan.state === 'clean'" :size="14" :stroke-width="2.25" />
              <Info v-else-if="scan.state === 'attention'" :size="14" :stroke-width="2" />
              <Minus v-else-if="scan.state === 'notApplicable' || scan.state === 'skipped'" :size="14" :stroke-width="2" />
              <TriangleAlert v-else-if="scan.state === 'failed'" :size="14" :stroke-width="2" />
              {{ stateLabel(scan) }}
              <em v-if="scan.reason && scan.state !== 'issues'">· {{ reasonText(scan.reason, t, te) }}</em>
            </span>
            <span class="section-size">{{ careBytes(scan.items) > 0 ? formatBytes(careBytes(scan.items)) : '' }}</span>
            <ChevronRight v-if="scan.items.length" class="chev" :size="16" :stroke-width="2" />
          </button>
        </header>

        <ul v-if="scan.items.length && !collapsed.has(scan.category)" class="items">
          <li
            v-for="item in scan.items"
            :key="careItemKey(item)"
            class="item"
            :data-disabled="itemDisabled(item)"
            :data-open="expanded.has(careItemKey(item))"
          >
           <div class="item-row">
            <input
              v-if="item.fixable"
              type="checkbox"
              class="check"
              :checked="!!store.selection[careItemKey(item)]"
              :disabled="itemDisabled(item)"
              :aria-label="itemTitle(item, t, te)"
              @change="store.setItemSelected(item, !store.selection[careItemKey(item)])"
            />
            <span v-else class="check-spacer" />
            <button
              type="button"
              class="item-copy"
              :aria-expanded="expanded.has(careItemKey(item))"
              :title="t('care.engine.details.toggle')"
              @click="toggleItem(item)"
            >
              <strong>{{ itemTitle(item, t, te) }}</strong>
              <span v-if="itemDetail(item, t, te) || item.location" class="item-detail">
                {{ itemDetail(item, t, te) }}
                <code v-if="item.location">{{ item.location }}</code>
              </span>
            </button>
            <span class="item-meta">
              <span v-if="item.count > 1" class="item-count">{{ t('care.engine.results.items', { count: item.count.toLocaleString() }, item.count) }}</span>
              <span v-if="item.bytes" class="item-size">{{ formatBytes(item.bytes) }}</span>
              <span v-if="item.needsAdmin" class="admin" :title="t('care.engine.badges.adminHint')">
                <ShieldAlert :size="12" :stroke-width="2" />
                {{ t('care.engine.badges.admin') }}
              </span>
              <PtStatusBadge :label="t(`care.engine.risk.${item.risk}`)" :tone="riskTone(item.risk)" />
              <button
                v-if="isOpenAction(item.action)"
                type="button"
                class="pt-btn pt-btn-ghost action-btn"
                @click="store.openAction(item.action!)"
              >
                {{ actionLabel(item.action!, t, te) }}
              </button>
              <button
                type="button"
                class="expand-btn"
                :aria-label="t('care.engine.details.toggle')"
                :aria-expanded="expanded.has(careItemKey(item))"
                @click="toggleItem(item)"
              >
                <ChevronDown :size="16" :stroke-width="2" />
              </button>
            </span>
           </div>

            <div v-if="expanded.has(careItemKey(item))" class="item-details">
              <dl>
                <template v-if="itemDetail(item, t, te)">
                  <dt>{{ t('care.engine.details.problem') }}</dt>
                  <dd>{{ itemDetail(item, t, te) }}</dd>
                </template>
                <template v-for="fact in itemFacts(item)" :key="fact.key">
                  <dt>{{ fact.label }}</dt>
                  <dd :class="{ mono: fact.mono, note: fact.key === 'running' }">{{ fact.value }}</dd>
                </template>
                <template v-if="item.location">
                  <dt>{{ t('care.engine.details.location') }}</dt>
                  <dd class="location">
                    <code>{{ item.location }}</code>
                    <button type="button" class="copy-btn" @click="copyLocation(item)">
                      <Check v-if="copiedKey === careItemKey(item)" :size="13" :stroke-width="2.25" />
                      <Copy v-else :size="13" :stroke-width="2" />
                      {{ copiedKey === careItemKey(item) ? t('care.engine.details.copied') : t('care.engine.details.copy') }}
                    </button>
                  </dd>
                </template>
                <template v-if="item.bytes || item.count > 1">
                  <dt>{{ t('care.engine.details.size') }}</dt>
                  <dd>
                    <template v-if="item.bytes">{{ formatBytes(item.bytes) }}</template>
                    <template v-if="item.bytes && item.count > 1"> · </template>
                    <template v-if="item.count > 1">{{ t('care.engine.results.items', { count: item.count.toLocaleString() }, item.count) }}</template>
                  </dd>
                </template>
                <dt>{{ t('care.engine.details.whatFixDoes') }}</dt>
                <dd>{{ fixText(item) }}</dd>
                <dt>{{ t('care.engine.details.risk') }}</dt>
                <dd>{{ t(`care.engine.details.riskMeaning.${item.risk}`) }}</dd>
                <template v-if="item.needsAdmin">
                  <dt>{{ t('care.engine.details.admin') }}</dt>
                  <dd>
                    {{ store.report?.isAdmin ? t('care.engine.details.adminOk') : t('care.engine.details.adminNeeded') }}
                  </dd>
                </template>
              </dl>
            </div>
          </li>
        </ul>
      </section>
    </div>

    <footer class="action-bar">
      <span class="selected-size">
        {{ store.selectedBytes > 0 ? t('care.engine.results.selectedSize', { size: formatBytes(store.selectedBytes) }) : '' }}
      </span>
      <button type="button" class="pt-btn pt-btn-ghost" @click="store.backToHome()">
        {{ t('care.engine.results.back') }}
      </button>
      <button type="button" class="pt-btn pt-btn-primary" :disabled="!selectedCount" @click="emit('fix')">
        <Wand2 :size="16" :stroke-width="2" />
        {{ selectedCount ? t('care.engine.results.fixCount', { count: selectedCount }, selectedCount) : t('care.engine.results.fix') }}
      </button>
    </footer>
  </div>
</template>

<style scoped>
.care-results {
  display: flex;
  height: 100%;
  min-height: 0;
  flex-direction: column;
  gap: 10px;
  animation: care-fade 200ms ease-out;
}
.toolbar {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
}
.totals {
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.totals strong {
  font-size: 1.125rem;
  font-weight: 700;
  letter-spacing: -0.02em;
}
.totals span {
  color: var(--muted-foreground);
  font-size: 0.75rem;
}
.toolbar-actions {
  display: flex;
  gap: 8px;
}
.notice {
  display: flex;
  align-items: center;
  gap: 8px;
  margin: 0;
  border-radius: 10px;
  background: color-mix(in oklab, var(--warning) 12%, transparent);
  color: var(--warning);
  padding: 8px 12px;
  font-size: 0.75rem;
  font-weight: 600;
}
.all-clean {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 4px;
  padding: 14px;
  border-radius: 14px;
  background: color-mix(in oklab, var(--success) 8%, var(--card));
  font-size: 0.8125rem;
}
.all-clean span:last-child {
  color: var(--muted-foreground);
}
.all-clean-icon {
  display: grid;
  width: 40px;
  height: 40px;
  place-items: center;
  border-radius: 50%;
  background: color-mix(in oklab, var(--success) 18%, transparent);
  color: var(--success);
}
.sections {
  display: flex;
  min-height: 0;
  flex: 1;
  flex-direction: column;
  gap: 8px;
  overflow: auto;
  padding-right: 2px;
}
.section {
  flex: none;
  border: 1px solid color-mix(in oklab, var(--border) 70%, transparent);
  border-radius: 12px;
  background: var(--card);
  overflow: hidden;
}
.section-head {
  display: flex;
  align-items: center;
  gap: 8px;
  min-height: 48px;
  padding: 0 12px;
}
.check,
.check-spacer {
  width: 15px;
  height: 15px;
  flex: none;
  accent-color: var(--primary);
}
.section-toggle {
  display: flex;
  min-width: 0;
  flex: 1;
  align-items: center;
  gap: 10px;
  min-height: 46px;
  border: 0;
  background: transparent;
  color: var(--foreground);
  text-align: left;
  cursor: pointer;
  font: inherit;
}
.section-toggle:disabled {
  cursor: default;
}
.section-icon {
  display: grid;
  width: 26px;
  height: 26px;
  flex: none;
  place-items: center;
  border-radius: 8px;
  background: color-mix(in oklab, var(--primary) 10%, transparent);
  color: var(--primary);
}
.section-name {
  font-size: 0.875rem;
  font-weight: 650;
}
.section-state {
  display: inline-flex;
  min-width: 0;
  align-items: center;
  gap: 5px;
  overflow: hidden;
  color: var(--muted-foreground);
  font-size: 0.75rem;
  font-weight: 600;
  white-space: nowrap;
  text-overflow: ellipsis;
}
.section-state em {
  font-style: normal;
  font-weight: 500;
}
.section-state[data-state='issues'] {
  color: var(--primary);
}
.section-state[data-state='clean'] {
  color: var(--success);
}
.section-state[data-state='attention'] {
  color: var(--warning);
}
.section-state[data-state='failed'] {
  color: var(--destructive);
}
.section-size {
  margin-left: auto;
  color: var(--muted-foreground);
  font-size: 0.75rem;
  font-variant-numeric: tabular-nums;
}
.chev {
  flex: none;
  color: var(--muted-foreground);
  transition: rotate 0.18s ease;
}
.section[data-open='true'] .chev {
  rotate: 90deg;
}
.items {
  margin: 0;
  padding: 0 0 4px;
  list-style: none;
  border-top: 1px solid color-mix(in oklab, var(--border) 55%, transparent);
}
.item {
  font-size: 0.8125rem;
}
.item-row {
  display: flex;
  align-items: center;
  gap: 10px;
  min-height: var(--layout-result-row-height, 44px);
  padding: 6px 12px 6px 12px;
  transition: background-color 0.12s ease;
}
.item-row:hover,
.item[data-open='true'] .item-row {
  background: color-mix(in oklab, var(--muted) 60%, transparent);
}
.expand-btn {
  display: grid;
  width: 28px;
  height: 28px;
  flex: none;
  place-items: center;
  border: 0;
  border-radius: 8px;
  background: transparent;
  color: var(--muted-foreground);
  cursor: pointer;
}
.expand-btn:hover {
  background: var(--muted);
  color: var(--foreground);
}
.expand-btn svg {
  transition: rotate 180ms ease;
}
.item[data-open='true'] .expand-btn svg {
  rotate: 180deg;
}
.item-details {
  margin: 0 12px 8px 40px;
  padding: 10px 14px;
  border: 1px solid color-mix(in oklab, var(--border) 65%, transparent);
  border-radius: 10px;
  background: var(--surface-soft);
  animation: details-in 160ms ease-out;
}
.item-details dl {
  display: grid;
  grid-template-columns: max-content minmax(0, 1fr);
  gap: 6px 16px;
  margin: 0;
}
.item-details dt {
  color: var(--muted-foreground);
  font-size: 0.75rem;
  font-weight: 600;
}
.item-details dd {
  margin: 0;
  color: var(--foreground);
  font-size: 0.78rem;
  line-height: 1.45;
  overflow-wrap: anywhere;
}
.item-details .mono,
.item-details code {
  font-family: ui-monospace, 'Cascadia Code', Consolas, monospace;
  font-size: 0.72rem;
}
.item-details .note {
  color: var(--warning);
  font-weight: 600;
}
.item-details .location {
  display: flex;
  flex-wrap: wrap;
  align-items: flex-start;
  gap: 8px;
}
.item-details .location code {
  min-width: 0;
  flex: 1;
  user-select: text;
}
.copy-btn {
  display: inline-flex;
  flex: none;
  align-items: center;
  gap: 4px;
  padding: 2px 8px;
  border: 1px solid color-mix(in oklab, var(--border) 85%, transparent);
  border-radius: 7px;
  background: var(--card);
  color: var(--muted-foreground);
  font-size: 0.6875rem;
  font-weight: 600;
  cursor: pointer;
}
.copy-btn:hover {
  color: var(--foreground);
}
@keyframes details-in {
  from {
    opacity: 0;
  }
}
@media (prefers-reduced-motion: reduce) {
  .item-details,
  .expand-btn svg {
    animation: none;
    transition: none;
  }
}
.item[data-disabled='true'] .item-copy {
  opacity: 0.75;
}
.item-copy {
  display: flex;
  min-width: 0;
  flex: 1;
  flex-direction: column;
  gap: 1px;
  padding: 0;
  border: 0;
  background: transparent;
  color: inherit;
  font: inherit;
  text-align: left;
  cursor: pointer;
}
.item-copy strong {
  overflow: hidden;
  font-weight: 600;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.item-detail {
  overflow: hidden;
  color: var(--muted-foreground);
  font-size: 0.75rem;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.item-detail code {
  margin-left: 6px;
  font-family: ui-monospace, 'Cascadia Code', Consolas, monospace;
  font-size: 0.6875rem;
}
.item-meta {
  display: flex;
  flex: none;
  align-items: center;
  gap: 8px;
}
.item-count,
.item-size {
  color: var(--muted-foreground);
  font-size: 0.75rem;
  font-variant-numeric: tabular-nums;
}
.item-size {
  min-width: 64px;
  text-align: right;
  color: var(--foreground);
  font-weight: 600;
}
.admin {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  height: 22px;
  padding: 0 8px;
  border-radius: 999px;
  background: color-mix(in oklab, var(--warning) 14%, transparent);
  color: var(--warning);
  font-size: 0.6875rem;
  font-weight: 600;
}
.action-btn {
  min-height: 30px;
  padding: 0 10px;
  font-size: 0.75rem;
}
.action-bar {
  display: flex;
  flex: none;
  align-items: center;
  justify-content: flex-end;
  gap: 10px;
  min-height: var(--layout-action-bar-height, 56px);
}
.selected-size {
  margin-right: auto;
  color: var(--muted-foreground);
  font-size: 0.8125rem;
}
@keyframes care-fade {
  from {
    opacity: 0;
  }
  to {
    opacity: 1;
  }
}
@media (prefers-reduced-motion: reduce) {
  .care-results {
    animation: none;
  }
  .chev {
    transition: none;
  }
}
.mode {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  color: var(--muted-foreground);
  font-size: 0.75rem;
  font-weight: 600;
}
.mode > svg {
  color: var(--primary);
}
.mode :deep(.pt-soft-select),
.mode > :last-child {
  min-width: 190px;
}
.mode-hint {
  display: flex;
  align-items: center;
  gap: 6px;
  margin: -4px 0 0;
  color: var(--muted-foreground);
  font-size: 0.75rem;
}
.mode-hint svg {
  flex: none;
  color: var(--primary);
}
.mode-hint[data-mode='all'] svg {
  color: var(--warning);
}
</style>

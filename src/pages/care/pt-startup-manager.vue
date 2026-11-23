<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue';
import { useI18n } from 'vue-i18n';
import {
  CircleAlert,
  EllipsisVertical,
  FolderOpen,
  Gauge,
  RefreshCw,
  Rocket,
  Search,
  Timer,
  TimerReset,
  Trash2,
} from '@lucide/vue';
import PtOperationWorkspace from '@/components/custom/pt-operation-workspace.vue';
import PtEmptyState from '@/components/custom/pt-empty-state.vue';
import PtConfirmDialog from '@/components/custom/pt-confirm-dialog.vue';
import { STARTUP_FILTERS, isPolicyLocked, startsWithWindows, type StartupItem } from '@/lib/models/startup';
import { useStartupStore } from '@/stores/startup-store';

const { t, locale } = useI18n();
const store = useStartupStore();

const openMenuId = ref<string | null>(null);
const deleteTarget = ref<StartupItem | null>(null);
const deleteOpen = ref(false);

function onDocumentPointer(event: PointerEvent) {
  const target = event.target as HTMLElement | null;
  if (!target?.closest('.row-menu')) openMenuId.value = null;
}
function onKey(event: KeyboardEvent) {
  if (event.key === 'Escape') openMenuId.value = null;
}
onMounted(() => {
  const sources = ['run', 'runOnce', 'startupFolder', 'scheduledTask', 'service', 'packagedApp'] as const;
  store.sourceLabels = Object.fromEntries(sources.map((s) => [s, t(`care.startup.sources.${s}`)]));
  document.addEventListener('pointerdown', onDocumentPointer);
  document.addEventListener('keydown', onKey);
  void store.ensureLoaded();
});
onBeforeUnmount(() => {
  document.removeEventListener('pointerdown', onDocumentPointer);
  document.removeEventListener('keydown', onKey);
});

const showLoading = computed(() => store.loading && !store.loaded);

const bootHeadline = computed(() => {
  const boot = store.boot;
  if (boot.access === 'available' && boot.lastBootMs != null) {
    return t('care.startup.boot.lastBoot', { seconds: Math.round(boot.lastBootMs / 1000) });
  }
  if (boot.access === 'needsAdmin') return t('care.startup.boot.needsAdmin');
  return t('care.startup.boot.unavailable');
});

const bootDetail = computed(() => {
  const at = store.boot.lastBootAtMs;
  if (store.boot.access !== 'available' || at == null) return t('care.startup.boot.impactUnknownHint');
  let when = '';
  try {
    when = new Intl.DateTimeFormat(locale.value || undefined, { dateStyle: 'medium', timeStyle: 'short' }).format(
      new Date(at)
    );
  } catch {
    when = '';
  }
  return t('care.startup.boot.measuredAt', { when, count: store.boot.bootsRecorded }, store.boot.bootsRecorded);
});

const totalsLine = computed(() =>
  t('care.startup.summary', {
    enabled: store.enabledCount,
    total: store.items.length,
    high: store.highImpactCount,
  })
);

const noticeText = computed(() => {
  const notice = store.notice;
  if (!notice) return '';
  return t(`care.startup.notice.${notice.status}`, {
    name: notice.name,
    action: t(`care.startup.actionNames.${notice.action}`),
  });
});

function initials(name: string): string {
  const words = name.replace(/[^\p{L}\p{N} ]/gu, ' ').trim().split(/\s+/).filter(Boolean);
  return ((words[0]?.[0] ?? '?') + (words[1]?.[0] ?? '')).toUpperCase();
}

function impactLabel(item: StartupItem): string {
  if (item.impactMs != null && (item.impact === 'high' || item.impact === 'medium' || item.impact === 'low')) {
    return t(`care.startup.impact.${item.impact}`) + ` · ${(item.impactMs / 1000).toFixed(1)} s`;
  }
  return t(`care.startup.impact.${item.impact}`);
}

function stateLabel(item: StartupItem): string {
  if (item.source === 'service' && item.serviceStart) {
    return t(`care.startup.serviceStart.${item.serviceStart}`);
  }
  return t(`care.startup.states.${item.state}`);
}

function toggleMenu(id: string) {
  openMenuId.value = openMenuId.value === id ? null : id;
}

function menuAction(item: StartupItem, action: 'delay' | 'undelay' | 'reveal' | 'delete') {
  openMenuId.value = null;
  if (action === 'reveal') {
    void store.reveal(item);
  } else if (action === 'delete') {
    deleteTarget.value = item;
    deleteOpen.value = true;
  } else {
    void store.run(item, action);
  }
}

function confirmDelete() {
  if (deleteTarget.value) void store.run(deleteTarget.value, 'delete');
  deleteTarget.value = null;
}

const adminPromptOpen = computed({
  get: () => store.adminPromptItem !== null,
  set: (open: boolean) => {
    if (!open) store.closeAdminPrompt();
  },
});
</script>

<template>
  <div class="startup">
    <PtOperationWorkspace
      v-if="showLoading"
      :status="t('care.startup.loading.status')"
      :title="t('care.startup.loading.title')"
      :hint="t('care.startup.loading.hint')"
      :icon="Rocket"
      indeterminate
      :cancellable="false"
    />

    <template v-else>
      <section class="overview">
        <div class="boot">
          <span class="boot-icon" aria-hidden="true"><Gauge :size="18" :stroke-width="1.9" /></span>
          <div class="boot-copy">
            <strong>{{ bootHeadline }}</strong>
            <span>{{ bootDetail }}</span>
          </div>
        </div>
        <div class="overview-side">
          <span class="totals">{{ totalsLine }}</span>
          <button type="button" class="pt-btn pt-btn-ghost small" :disabled="store.busy" @click="store.load()">
            <RefreshCw :size="14" :stroke-width="2" aria-hidden="true" />
            {{ t('care.startup.refresh') }}
          </button>
        </div>
      </section>

      <div class="toolbar">
        <label class="search">
          <Search :size="15" :stroke-width="2" aria-hidden="true" />
          <input
            v-model="store.query"
            type="search"
            :placeholder="t('care.startup.searchPlaceholder')"
            :aria-label="t('care.startup.searchPlaceholder')"
          />
        </label>
        <nav class="filters" role="tablist">
          <button
            v-for="filter in STARTUP_FILTERS"
            :key="filter"
            type="button"
            role="tab"
            class="chip"
            :class="{ active: store.filter === filter }"
            :aria-selected="store.filter === filter"
            @click="store.filter = filter"
          >
            {{ t(`care.startup.filters.${filter}`) }}
            <span class="chip-count">{{ store.filterCounts[filter] }}</span>
          </button>
        </nav>
        <label class="ms-toggle" :title="t('care.startup.showMicrosoftHint')">
          <input
            type="checkbox"
            :checked="store.includeMicrosoft"
            :disabled="store.busy"
            @change="store.setIncludeMicrosoft(($event.target as HTMLInputElement).checked)"
          />
          {{ t('care.startup.showMicrosoft') }}
          <span v-if="!store.includeMicrosoft && store.hiddenMicrosoftCount" class="chip-count">
            {{ store.hiddenMicrosoftCount }}
          </span>
        </label>
      </div>

      <p v-if="store.notice" class="notice" :data-status="store.notice.status" role="status" aria-live="polite">
        <span>{{ noticeText }}</span>
        <button type="button" class="text-btn" @click="store.dismissNotice()">{{ t('care.startup.dismiss') }}</button>
      </p>

      <div class="list-card">
        <div class="list-head">
          <span>{{ t('care.startup.columns.name') }}</span>
          <span>{{ t('care.startup.columns.source') }}</span>
          <span>{{ t('care.startup.columns.impact') }}</span>
          <span>{{ t('care.startup.columns.status') }}</span>
          <span />
        </div>
        <ul v-if="store.visibleItems.length" class="rows">
          <li
            v-for="item in store.visibleItems"
            :key="item.id"
            class="row"
            :class="{ off: !startsWithWindows(item), pending: store.pendingIds.includes(item.id) }"
          >
            <div class="name-cell">
              <span
                class="icon"
                :class="{ initials: !store.icons[item.id], plated: item.source === 'packagedApp' && store.icons[item.id] }"
                aria-hidden="true"
              >
                <img v-if="store.icons[item.id]" :src="store.icons[item.id] ?? undefined" alt="" draggable="false" />
                <template v-else>{{ initials(item.name) }}</template>
              </span>
              <span class="name-copy">
                <span class="name-line">
                  <strong :title="item.name">{{ item.name }}</strong>
                  <span v-if="item.isNew" class="badge badge-new">{{ t('care.startup.badges.new') }}</span>
                  <span v-if="item.broken" class="badge badge-broken" :title="t('care.startup.badgeHints.broken')">
                    {{ t('care.startup.badges.broken') }}
                  </span>
                  <span
                    v-if="item.requiresAdmin && !store.isAdmin && (item.canToggle || item.canDelete)"
                    class="badge badge-admin"
                    :title="t('care.startup.badgeHints.admin')"
                  >{{ t('care.startup.badges.admin') }}</span>
                  <span v-if="isPolicyLocked(item)" class="badge badge-admin" :title="t('care.startup.badgeHints.policy')">
                    {{ t('care.startup.badges.policy') }}
                  </span>
                  <span v-else-if="item.isMicrosoft && !item.canToggle" class="badge badge-admin">
                    {{ t('care.startup.badges.system') }}
                  </span>
                </span>
                <small :title="item.command">{{ item.publisher || t('care.startup.unknownPublisher') }}</small>
              </span>
            </div>
            <span class="source">
              <span class="source-chip">{{ t(`care.startup.sources.${item.source}`) }}</span>
              <small>{{ t(`care.startup.scopes.${item.scope}`) }}</small>
            </span>
            <span class="impact" :data-impact="item.impact">{{ impactLabel(item) }}</span>
            <span class="status">
              <button
                type="button"
                role="switch"
                class="switch"
                :class="{ on: startsWithWindows(item) }"
                :aria-checked="startsWithWindows(item)"
                :aria-label="t('care.startup.toggleLabel', { name: item.name })"
                :disabled="!item.canToggle || store.pendingIds.includes(item.id) || store.loading"
                @click="store.toggle(item)"
              >
                <span class="knob" />
              </button>
              <span class="status-text">
                <Timer v-if="item.state === 'delayed'" :size="12" :stroke-width="2" aria-hidden="true" />
                {{ stateLabel(item) }}
              </span>
            </span>
            <span class="row-menu">
              <button
                type="button"
                class="kebab"
                :aria-label="t('care.startup.moreActions')"
                :aria-expanded="openMenuId === item.id"
                :disabled="store.pendingIds.includes(item.id)"
                @click="toggleMenu(item.id)"
              >
                <EllipsisVertical :size="16" :stroke-width="2" aria-hidden="true" />
              </button>
              <div v-if="openMenuId === item.id" class="menu" role="menu">
                <button
                  v-if="item.canDelay && item.state !== 'delayed'"
                  type="button"
                  role="menuitem"
                  @click="menuAction(item, 'delay')"
                >
                  <Timer :size="14" :stroke-width="2" aria-hidden="true" />
                  {{ item.source === 'service' ? t('care.startup.menu.delayService') : t('care.startup.menu.delay') }}
                </button>
                <button v-if="item.state === 'delayed' && item.canToggle" type="button" role="menuitem" @click="menuAction(item, 'undelay')">
                  <TimerReset :size="14" :stroke-width="2" aria-hidden="true" />
                  {{ t('care.startup.menu.undelay') }}
                </button>
                <button v-if="item.targetPath && !item.broken" type="button" role="menuitem" @click="menuAction(item, 'reveal')">
                  <FolderOpen :size="14" :stroke-width="2" aria-hidden="true" />
                  {{ t('care.startup.menu.reveal') }}
                </button>
                <button v-if="item.canDelete" type="button" role="menuitem" class="danger" @click="menuAction(item, 'delete')">
                  <Trash2 :size="14" :stroke-width="2" aria-hidden="true" />
                  {{ t('care.startup.menu.delete') }}
                </button>
                <p class="menu-command" :title="item.command">{{ item.command }}</p>
              </div>
            </span>
          </li>
        </ul>
        <PtEmptyState
          v-else-if="store.loaded"
          :title="t('care.startup.emptyTitle')"
          :description="t('care.startup.emptyBody')"
        />
      </div>

      <p v-if="store.boot.access === 'needsAdmin' && !store.isAdmin" class="hint">
        <CircleAlert :size="13" :stroke-width="2" aria-hidden="true" />
        {{ t('care.startup.boot.adminHint') }}
        <button type="button" class="text-btn" :disabled="store.restartingElevated" @click="store.restartAsAdministrator()">
          {{ t('care.startup.restartAdmin') }}
        </button>
      </p>
    </template>

    <PtConfirmDialog
      v-model:open="deleteOpen"
      :title="t('care.startup.deleteConfirm.title')"
      :message="t('care.startup.deleteConfirm.message', { name: deleteTarget?.name ?? '' })"
      :confirm-text="t('care.startup.deleteConfirm.confirm')"
      :cancel-text="t('care.startup.cancel')"
      destructive
      @confirm="confirmDelete"
      @cancel="deleteTarget = null"
      @dismiss="deleteTarget = null"
    />

    <PtConfirmDialog
      v-model:open="adminPromptOpen"
      :title="t('care.startup.adminPrompt.title')"
      :message="t('care.startup.adminPrompt.message', { name: store.adminPromptItem?.name ?? '' })"
      :confirm-text="t('care.startup.restartAdmin')"
      :cancel-text="t('care.startup.adminPrompt.decline')"
      @confirm="store.restartAsAdministrator()"
    />
  </div>
</template>

<style scoped>
.startup {
  display: flex;
  min-height: 0;
  flex: 1;
  flex-direction: column;
  gap: 10px;
}
.overview {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  justify-content: space-between;
  gap: 10px 16px;
  padding: 12px 14px;
  border: 1px solid color-mix(in oklab, var(--border) 75%, transparent);
  border-radius: 14px;
  background: var(--card);
}
.boot {
  display: flex;
  min-width: 0;
  align-items: center;
  gap: 12px;
}
.boot-icon {
  display: grid;
  width: 36px;
  height: 36px;
  flex: none;
  place-items: center;
  border-radius: 10px;
  background: color-mix(in oklab, var(--primary) 12%, transparent);
  color: var(--primary);
}
.boot-copy {
  display: flex;
  min-width: 0;
  flex-direction: column;
}
.boot-copy strong {
  font-size: 0.9375rem;
  font-weight: 700;
}
.boot-copy span,
.totals {
  color: var(--muted-foreground);
  font-size: 0.75rem;
}
.overview-side {
  display: flex;
  align-items: center;
  gap: 12px;
}
.small {
  min-height: 32px;
  padding: 0 12px;
  font-size: 0.75rem;
}
.toolbar {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 10px;
}
.search {
  display: flex;
  min-width: 220px;
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
.filters {
  display: flex;
  flex-wrap: wrap;
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
.ms-toggle {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  color: var(--muted-foreground);
  font-size: 0.75rem;
  font-weight: 600;
  cursor: pointer;
}
.notice {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  margin: 0;
  padding: 8px 12px;
  border-radius: 10px;
  background: color-mix(in oklab, var(--success) 12%, var(--card));
  font-size: 0.75rem;
}
.notice[data-status='skippedNeedAdmin'],
.notice[data-status='notFound'],
.notice[data-status='unsupported'] {
  background: color-mix(in oklab, var(--warning) 12%, var(--card));
}
.notice[data-status='failed'] {
  background: color-mix(in oklab, var(--destructive) 12%, var(--card));
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
  grid-template-columns: minmax(0, 1fr) 132px 128px 150px 40px;
  align-items: center;
  gap: 8px;
  padding: 0 12px;
}
.list-head {
  height: 36px;
  padding: 0 16px;
  overflow-y: hidden;
  scrollbar-gutter: stable;
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
.row.off .name-copy,
.row.off .icon {
  opacity: 0.62;
}
.row.pending {
  opacity: 0.7;
}
.name-cell {
  display: flex;
  min-width: 0;
  align-items: center;
  gap: 10px;
}
.icon {
  display: grid;
  width: 30px;
  height: 30px;
  flex: none;
  place-items: center;
  overflow: hidden;
  border-radius: 8px;
}
.icon img {
  width: 100%;
  height: 100%;
  object-fit: contain;
}
/* Store logos are often white-on-transparent; draw them on a tile like Windows does. */
.icon.plated {
  padding: 4px;
  background: color-mix(in oklab, var(--foreground) 62%, var(--card));
}
.icon.initials {
  background: color-mix(in oklab, var(--primary) 14%, var(--card));
  color: var(--primary);
  font-size: 0.6875rem;
  font-weight: 750;
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
.name-copy small,
.source small {
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
  font-size: 0.625rem;
  font-weight: 700;
}
.badge-new {
  background: color-mix(in oklab, var(--primary) 14%, transparent);
  color: var(--primary);
}
.badge-broken {
  background: color-mix(in oklab, var(--warning) 18%, transparent);
  color: var(--warning);
}
.badge-admin {
  background: var(--muted);
  color: var(--muted-foreground);
}
.source {
  display: flex;
  min-width: 0;
  flex-direction: column;
  align-items: flex-start;
  gap: 2px;
}
.source-chip {
  padding: 1px 8px;
  border-radius: 999px;
  background: var(--muted);
  color: var(--foreground);
  font-size: 0.6875rem;
  font-weight: 600;
  white-space: nowrap;
}
.impact {
  color: var(--muted-foreground);
  font-size: 0.75rem;
  font-weight: 600;
  white-space: nowrap;
}
.impact[data-impact='high'] {
  color: var(--destructive);
}
.impact[data-impact='medium'] {
  color: var(--warning);
}
.impact[data-impact='low'] {
  color: var(--success);
}
.status {
  display: flex;
  align-items: center;
  gap: 8px;
}
.status-text {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  color: var(--muted-foreground);
  font-size: 0.75rem;
  white-space: nowrap;
}
.switch {
  position: relative;
  width: 34px;
  height: 20px;
  flex: none;
  padding: 0;
  border: 1px solid color-mix(in oklab, var(--border) 85%, transparent);
  border-radius: 999px;
  background: var(--muted);
  cursor: pointer;
  transition:
    background-color 160ms ease,
    border-color 160ms ease;
}
.switch .knob {
  position: absolute;
  top: 2px;
  left: 2px;
  width: 14px;
  height: 14px;
  border-radius: 50%;
  background: var(--card);
  box-shadow: 0 1px 2px rgb(0 0 0 / 18%);
  transition: left 160ms ease;
}
.switch.on {
  border-color: var(--primary);
  background: var(--primary);
}
.switch.on .knob {
  left: 16px;
}
.switch:disabled {
  cursor: default;
  opacity: 0.45;
}
.row-menu {
  position: relative;
  display: flex;
  justify-content: flex-end;
}
.kebab {
  display: grid;
  width: 30px;
  height: 30px;
  place-items: center;
  border: 1px solid transparent;
  border-radius: 8px;
  background: transparent;
  color: var(--muted-foreground);
  cursor: pointer;
}
.kebab:hover:not(:disabled),
.kebab[aria-expanded='true'] {
  border-color: color-mix(in oklab, var(--border) 80%, transparent);
  background: var(--card);
  color: var(--foreground);
}
.menu {
  position: absolute;
  top: 34px;
  right: 0;
  z-index: 20;
  display: flex;
  width: 248px;
  flex-direction: column;
  gap: 2px;
  padding: 6px;
  border: 1px solid color-mix(in oklab, var(--border) 80%, transparent);
  border-radius: 12px;
  background: var(--card);
  box-shadow: var(--shadow-card);
}
.menu button {
  display: flex;
  align-items: center;
  gap: 8px;
  min-height: 32px;
  padding: 0 10px;
  border: 0;
  border-radius: 8px;
  background: transparent;
  color: var(--foreground);
  font-size: 0.8125rem;
  text-align: left;
  cursor: pointer;
}
.menu button:hover {
  background: var(--muted);
}
.menu button.danger {
  color: var(--destructive);
}
.menu-command {
  margin: 4px 0 0;
  padding: 6px 10px 2px;
  overflow: hidden;
  border-top: 1px solid color-mix(in oklab, var(--border) 65%, transparent);
  color: var(--muted-foreground);
  font-size: 0.6875rem;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.hint {
  display: flex;
  align-items: center;
  gap: 6px;
  margin: 0;
  color: var(--muted-foreground);
  font-size: 0.75rem;
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
    grid-template-columns: minmax(0, 1fr) 110px 132px 40px;
  }
  .list-head > :nth-child(2),
  .row > .source {
    display: none;
  }
}
@media (prefers-reduced-motion: reduce) {
  .switch,
  .switch .knob {
    transition: none;
  }
}
</style>

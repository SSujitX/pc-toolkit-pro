<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import type { Component } from 'vue';
import { useI18n } from 'vue-i18n';
import {
  AppWindow,
  Check,
  CircleAlert,
  Clock,
  FileSearch,
  FolderOpen,
  Loader2,
  ShieldAlert,
  ShieldCheck,
  Trash2,
  X,
} from '@lucide/vue';
import PtMiddleEllipsis from '@/components/custom/pt-middle-ellipsis.vue';
import PtProgramIcon from '@/pages/uninstaller/pt-program-icon.vue';
import {
  LEFTOVER_AREAS,
  type InstalledProgram,
  type LeftoverArea,
  type LeftoverItemStatus,
  type UninstallerProgress,
} from '@/lib/models/uninstaller';
import type { CleanLogEntry, UninstallerOperation } from '@/stores/uninstaller-store';

const props = defineProps<{
  operation: UninstallerOperation;
  progress: UninstallerProgress | null;
  program: InstalledProgram | null;
  iconSrc: string | null;
  queueIndex: number;
  queueTotal: number;
  restorePointRequested: boolean;
  forced: boolean;
  areasDone: LeftoverArea[];
  cleanLog: CleanLogEntry[];
  cleanTotal: number;
  /** Items processed across the whole batch (store-held, independent of the log cap). */
  cleanProcessed: number;
  /** Items that ended with an issue across the whole batch. */
  cleanIssues: number;
}>();

const emit = defineEmits<{ cancel: []; focusUninstaller: [] }>();
const { t } = useI18n();

type StepState = 'pending' | 'active' | 'done';
type StepId = 'restorePoint' | 'uninstall' | 'scan' | 'review';

const phase = computed(() => props.progress?.phase ?? (props.operation === 'cleaning' ? 'cleaning' : 'uninstalling'));
const cleaning = computed(() => props.operation === 'cleaning');
const scanning = computed(() => !cleaning.value && (phase.value === 'scanning' || props.operation === 'scanning'));
const waitingForUser = computed(() => phase.value === 'uninstalling' && !!props.progress?.windowVisible);

/* ---------- Smooth elapsed clock (interpolates between backend ticks) ---------- */
const now = ref(Date.now());
const lastEventAt = ref(Date.now());
const lastElapsedMs = ref(0);
let clock: number | null = null;
watch(
  () => props.progress,
  (p) => {
    lastEventAt.value = Date.now();
    lastElapsedMs.value = p?.elapsedMs ?? 0;
  },
  { immediate: true }
);
watch(
  () => props.operation,
  () => {
    lastEventAt.value = Date.now();
    lastElapsedMs.value = 0;
  }
);
onMounted(() => {
  clock = window.setInterval(() => (now.value = Date.now()), 250);
});
onBeforeUnmount(() => {
  if (clock !== null) window.clearInterval(clock);
  if (tweenFrame !== null) cancelAnimationFrame(tweenFrame);
});
const elapsedLabel = computed(() => {
  const s = Math.max(0, Math.floor((lastElapsedMs.value + (now.value - lastEventAt.value)) / 1000));
  return s >= 60 ? `${Math.floor(s / 60)}m ${String(s % 60).padStart(2, '0')}s` : `${s}s`;
});

/* ---------- Steps ---------- */
const steps = computed(() => {
  const ids: StepId[] = [];
  if (props.restorePointRequested && !props.forced) ids.push('restorePoint');
  if (!props.forced) ids.push('uninstall');
  ids.push('scan', 'review');
  const activeId: StepId = cleaning.value
    ? 'review'
    : phase.value === 'restorePoint'
      ? 'restorePoint'
      : scanning.value
        ? 'scan'
        : 'uninstall';
  const activeIndex = ids.indexOf(activeId);
  return ids.map((id, index) => ({
    id,
    label: t(`uninstaller.dialog.steps.${id}`),
    state: (index < activeIndex ? 'done' : index === activeIndex ? 'active' : 'pending') as StepState,
  }));
});

/* ---------- Header copy ---------- */
const statusKey = computed(() => {
  if (cleaning.value) return 'cleaning';
  if (phase.value === 'restorePoint') return 'restorePoint';
  if (scanning.value) return 'scanning';
  return waitingForUser.value ? 'waiting' : 'uninstalling';
});

const title = computed(() => {
  const name = props.program?.name ?? '';
  if (cleaning.value) return t('uninstaller.dialog.cleaningTitle');
  if (phase.value === 'restorePoint') return t('uninstaller.dialog.restorePointTitle');
  if (scanning.value) return t('uninstaller.dialog.scanningTitle', { name });
  return t('uninstaller.dialog.uninstallingTitle', { name });
});

/* ---------- Scan progress ---------- */
const areaRows = computed(() =>
  LEFTOVER_AREAS.map((area) => ({
    area,
    label: t(`uninstaller.progress.areas.${area}`),
    state: (props.areasDone.includes(area)
      ? 'done'
      : props.progress?.area === area
        ? 'active'
        : 'pending') as StepState,
  }))
);
const areasCompleted = computed(() => areaRows.value.filter((r) => r.state === 'done').length);
const scanPercent = computed(() => {
  const active = areaRows.value.some((r) => r.state === 'active') ? 0.5 : 0;
  return Math.min(98, Math.round(((areasCompleted.value + active) / LEFTOVER_AREAS.length) * 100));
});

/* ---------- Clean progress ---------- */
const cleanDone = computed(() => props.cleanProcessed);
const cleanPercent = computed(() =>
  props.cleanTotal ? Math.min(100, Math.round((cleanDone.value / props.cleanTotal) * 100)) : 0
);
const cleanIssues = computed(() => props.cleanIssues);

/* ---------- Animated counters ---------- */
const shownFound = ref(0);
const shownDone = ref(0);
let tweenFrame: number | null = null;
function tweenCounters() {
  const targetFound = props.progress?.itemsFound ?? 0;
  const targetDone = cleanDone.value;
  const step = (from: number, to: number) => {
    if (from === to) return to;
    const next = from + (to - from) * 0.25;
    return Math.abs(to - next) < 0.5 ? to : next;
  };
  shownFound.value = step(shownFound.value, scanning.value ? targetFound : shownFound.value);
  shownDone.value = step(shownDone.value, cleaning.value ? targetDone : 0);
  const settled =
    (!scanning.value || shownFound.value === targetFound) && (!cleaning.value || shownDone.value === targetDone);
  tweenFrame = settled ? null : requestAnimationFrame(tweenCounters);
}
watch([() => props.progress?.itemsFound, cleanDone, () => props.operation], () => {
  if (tweenFrame === null) tweenFrame = requestAnimationFrame(tweenCounters);
});

/* ---------- Bar + source card + stats ---------- */
const indeterminate = computed(() => !scanning.value && !cleaning.value);
const barPercent = computed(() => (cleaning.value ? cleanPercent.value : scanning.value ? scanPercent.value : 0));

const source = computed<{ icon: Component; label: string; value: string | null; text: string | null }>(() => {
  if (cleaning.value) {
    return {
      icon: Trash2,
      label: t('uninstaller.dialog.source.removing'),
      value: props.progress?.currentPath ?? null,
      text: props.progress?.currentPath ? null : t('uninstaller.dialog.source.starting'),
    };
  }
  if (scanning.value) {
    const area = props.progress?.area;
    return {
      icon: FileSearch,
      label: area
        ? `${t('uninstaller.dialog.source.scanning')} · ${t(`uninstaller.progress.areas.${area}`)}`
        : t('uninstaller.dialog.source.scanning'),
      value: props.progress?.currentPath ?? null,
      text: props.progress?.currentPath ? null : t('uninstaller.dialog.source.starting'),
    };
  }
  if (phase.value === 'restorePoint') {
    return { icon: ShieldCheck, label: t('uninstaller.dialog.source.restorePoint'), value: null, text: t('uninstaller.dialog.restorePointBody') };
  }
  return {
    icon: FolderOpen,
    label: t('uninstaller.dialog.source.uninstaller'),
    value: props.program?.installLocation || null,
    text: props.program?.installLocation ? null : t('uninstaller.dialog.runningBody'),
  };
});

const stats = computed(() => {
  const elapsed = { key: 'elapsed', label: t('uninstaller.dialog.stats.elapsed'), value: elapsedLabel.value };
  if (cleaning.value) {
    return [
      {
        key: 'done',
        label: t('uninstaller.dialog.stats.processed'),
        value: `${Math.round(shownDone.value)} / ${props.cleanTotal}`,
      },
      { key: 'issues', label: t('uninstaller.dialog.stats.issues'), value: String(cleanIssues.value) },
      elapsed,
    ];
  }
  if (scanning.value) {
    return [
      {
        key: 'areas',
        label: t('uninstaller.dialog.stats.areas'),
        value: `${areasCompleted.value} / ${LEFTOVER_AREAS.length}`,
      },
      { key: 'found', label: t('uninstaller.dialog.stats.found'), value: String(Math.round(shownFound.value)) },
      elapsed,
    ];
  }
  return [
    {
      key: 'state',
      label: t('uninstaller.dialog.stats.state'),
      value:
        phase.value === 'restorePoint'
          ? t('uninstaller.dialog.status.restorePoint')
          : waitingForUser.value
            ? t('uninstaller.dialog.stats.waiting')
            : t('uninstaller.dialog.stats.running'),
    },
    {
      key: 'procs',
      label: t('uninstaller.dialog.stats.processes'),
      value: phase.value === 'restorePoint' ? '—' : String(props.progress?.current ?? 0),
    },
    elapsed,
  ];
});

/* ---------- Clean log ---------- */
const statusIcons: Record<LeftoverItemStatus, Component> = {
  removed: Check,
  notFound: Check,
  pendingReboot: Clock,
  skippedNeedAdmin: ShieldAlert,
  backupFailed: CircleAlert,
  failed: X,
};

function statusTone(status: LeftoverItemStatus): 'ok' | 'warn' | 'bad' {
  if (status === 'removed' || status === 'notFound') return 'ok';
  if (status === 'pendingReboot' || status === 'skippedNeedAdmin') return 'warn';
  return 'bad';
}

const cancelLabel = computed(() =>
  phase.value === 'uninstalling' ? t('uninstaller.dialog.stopWaiting') : t('common.cancel')
);
const cancellable = computed(() => phase.value === 'uninstalling' || cleaning.value);
const hint = computed(() => {
  if (cleaning.value) return t('uninstaller.dialog.cleanHint');
  if (scanning.value) return t('uninstaller.dialog.scanHint');
  if (phase.value === 'restorePoint') return null;
  return t('uninstaller.dialog.autoContinue');
});
</script>

<template>
  <div class="dialog-root" role="dialog" aria-modal="true" :aria-label="title">
    <div class="overlay" />
    <div class="panel" :data-status="statusKey">
      <header class="head">
        <div
          class="ring-wrap"
          role="progressbar"
          :aria-valuenow="indeterminate ? undefined : barPercent"
          aria-valuemin="0"
          aria-valuemax="100"
          :aria-label="t(`uninstaller.dialog.status.${statusKey}`)"
        >
          <div class="ring-disc" aria-hidden="true" />
          <!-- Circular working spinner; progress is shown on the bar below. -->
          <div class="ring-spinner" aria-hidden="true" />
          <div class="ring-icon">
            <PtProgramIcon
              v-if="program && !cleaning"
              :name="program.name"
              :src="iconSrc"
              :plated="program.source === 'store'"
              :size="34"
            />
            <Trash2 v-else :size="20" :stroke-width="2" />
          </div>
        </div>
        <div class="head-copy">
          <span class="status">
            {{ t(`uninstaller.dialog.status.${statusKey}`) }}
            <template v-if="queueTotal > 1 && !cleaning">
              · {{ t('uninstaller.dialog.queue', { index: queueIndex, total: queueTotal }) }}
            </template>
          </span>
          <h2 :title="title">{{ title }}</h2>
          <small v-if="program && !cleaning && (program.publisher || program.version)">{{
            [program.publisher, program.version].filter(Boolean).join(' · ')
          }}</small>
        </div>
      </header>

      <ol class="steps">
        <li v-for="(step, index) in steps" :key="step.id" :data-state="step.state">
          <span v-if="index > 0" class="rail" aria-hidden="true"><span /></span>
          <span class="dot">
            <Check v-if="step.state === 'done'" class="pop" :size="11" :stroke-width="3.2" />
            <span v-else-if="step.state === 'active'" class="pulse" />
          </span>
          <span class="step-label">{{ step.label }}</span>
        </li>
      </ol>

      <section class="body">
        <Transition name="swap" mode="out-in">
          <div v-if="waitingForUser" key="waiting" class="callout">
            <span class="callout-glyph"><AppWindow :size="18" :stroke-width="1.9" /></span>
            <div>
              <strong>{{ t('uninstaller.dialog.waitingTitle') }}</strong>
              <p>{{ t('uninstaller.dialog.waitingBody', { name: program?.name ?? '' }) }}</p>
              <button type="button" class="pt-btn pt-btn-primary small" @click="emit('focusUninstaller')">
                {{ t('uninstaller.dialog.showUninstaller') }}
              </button>
            </div>
          </div>
          <div v-else :key="`source-${statusKey}`" class="source">
            <span class="source-glyph" aria-hidden="true">
              <component :is="source.icon" :size="15" :stroke-width="1.9" />
            </span>
            <div class="source-copy">
              <span class="source-label">{{ source.label }}</span>
              <code v-if="source.value"><PtMiddleEllipsis :text="source.value" /></code>
              <p v-else class="source-text">{{ source.text }}</p>
            </div>
          </div>
        </Transition>

        <div class="bar-row">
          <div class="bar" :class="{ 'bar--indeterminate': indeterminate }" aria-hidden="true">
            <span v-if="!indeterminate" class="fill" :style="{ width: `${Math.max(barPercent, 2)}%` }" />
            <span v-else class="sweep" />
          </div>
          <span v-if="!indeterminate" class="bar-pct">{{ barPercent }}%</span>
        </div>

        <div class="stats">
          <div v-for="stat in stats" :key="stat.key" class="stat">
            <span>{{ stat.label }}</span>
            <strong>{{ stat.value }}</strong>
          </div>
        </div>

        <Transition name="swap" mode="out-in">
          <ul v-if="scanning" key="areas" class="areas">
            <li v-for="row in areaRows" :key="row.area" :data-state="row.state">
              <Check v-if="row.state === 'done'" class="pop" :size="14" :stroke-width="2.6" />
              <Loader2 v-else-if="row.state === 'active'" class="spin" :size="14" :stroke-width="2.6" />
              <span v-else class="pending-dot" />
              {{ row.label }}
            </li>
          </ul>

          <TransitionGroup v-else-if="cleaning" key="log" tag="ul" name="log" class="log">
            <li v-for="entry in cleanLog" :key="entry.key" :data-tone="statusTone(entry.status)">
              <component :is="statusIcons[entry.status]" :size="13" :stroke-width="2.4" />
              <PtMiddleEllipsis class="path" :text="entry.location" />
              <em>{{ t(`uninstaller.dialog.itemStatus.${entry.status}`) }}</em>
            </li>
          </TransitionGroup>
        </Transition>

        <p v-if="hint" class="hint">{{ hint }}</p>
      </section>

      <footer v-if="cancellable" class="foot">
        <button type="button" class="cancel" @click="emit('cancel')">
          {{ cancelLabel }}
        </button>
      </footer>
    </div>
  </div>
</template>

<style scoped>
.dialog-root {
  position: fixed;
  inset: 0;
  z-index: 85;
  display: grid;
  place-items: center;
  padding: 16px;
}
.overlay {
  position: absolute;
  inset: 0;
  background: rgb(0 0 0 / 34%);
  animation: fade-in 180ms ease-out;
}
.panel {
  --accent: var(--primary);
  position: relative;
  z-index: 1;
  display: flex;
  width: min(540px, calc(100vw - 32px));
  max-height: min(680px, calc(100vh - 32px));
  flex-direction: column;
  border: 1px solid color-mix(in oklab, var(--border) 75%, transparent);
  border-radius: 18px;
  background: var(--card);
  box-shadow: 0 25px 50px -12px rgb(0 0 0 / 30%);
  padding: 22px 24px 20px;
  animation: panel-in 240ms cubic-bezier(0.22, 1, 0.36, 1);
}
.panel[data-status='waiting'] {
  --accent: var(--warning);
}

/* ---------- Header ring ---------- */
.head {
  display: flex;
  align-items: center;
  gap: 14px;
}
.ring-wrap {
  position: relative;
  width: 58px;
  height: 58px;
  flex: none;
  border-radius: 50%;
  isolation: isolate;
}
.ring-wrap::before {
  content: '';
  position: absolute;
  inset: 0;
  border: 3px solid color-mix(in oklab, var(--accent) 16%, var(--muted));
  border-radius: 50%;
  transition: border-color 300ms ease;
}
.ring-disc {
  position: absolute;
  inset: 4px;
  border-radius: 50%;
  background: color-mix(in oklab, var(--accent) 10%, var(--card));
  animation: ring-breathe 1.8s ease-in-out infinite;
  transition: background-color 300ms ease;
}
.ring-spinner {
  position: absolute;
  inset: 0;
  z-index: 1;
  border-radius: 50%;
  background: conic-gradient(
    from 0deg,
    var(--accent) 0deg,
    color-mix(in oklab, var(--accent) 50%, transparent) 80deg,
    transparent 125deg
  );
  -webkit-mask: radial-gradient(farthest-side, transparent calc(100% - 3px), #000 calc(100% - 2.5px));
  mask: radial-gradient(farthest-side, transparent calc(100% - 3px), #000 calc(100% - 2.5px));
  animation: ring-spin 0.75s linear infinite;
  pointer-events: none;
}
.panel[data-status='waiting'] .ring-spinner {
  animation-duration: 1.6s;
}
.ring-icon {
  position: absolute;
  inset: 0;
  z-index: 2;
  display: grid;
  place-items: center;
  color: var(--accent);
}
.head-copy {
  display: flex;
  min-width: 0;
  flex-direction: column;
  gap: 2px;
}
.status {
  color: var(--accent);
  font-size: 0.75rem;
  font-weight: 700;
  transition: color 300ms ease;
}
.head-copy h2 {
  margin: 0;
  overflow: hidden;
  font-size: 1.125rem;
  font-weight: 700;
  letter-spacing: -0.02em;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.head-copy small {
  overflow: hidden;
  color: var(--muted-foreground);
  font-size: 0.75rem;
  text-overflow: ellipsis;
  white-space: nowrap;
}

/* ---------- Step tracker ---------- */
.steps {
  display: flex;
  align-items: center;
  list-style: none;
  margin: 18px 0 0;
  padding: 0;
}
.steps li {
  display: flex;
  min-width: 0;
  flex: 1;
  align-items: center;
  gap: 6px;
  color: var(--muted-foreground);
  font-size: 0.75rem;
  font-weight: 600;
}
.steps li:first-child {
  flex: none;
}
.steps li[data-state='active'] {
  color: var(--foreground);
}
.step-label {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.rail {
  position: relative;
  min-width: 12px;
  height: 2px;
  flex: 1;
  margin: 0 4px 0 2px;
  overflow: hidden;
  border-radius: 999px;
  background: color-mix(in oklab, var(--border) 90%, transparent);
}
.rail span {
  position: absolute;
  inset: 0;
  border-radius: inherit;
  background: var(--success);
  transform: scaleX(0);
  transform-origin: left center;
  transition: transform 450ms cubic-bezier(0.22, 1, 0.36, 1);
}
.steps li[data-state='done'] .rail span {
  transform: scaleX(1);
}
.steps li[data-state='active'] .rail span {
  background: var(--primary);
  transform: scaleX(1);
}
.dot {
  position: relative;
  display: grid;
  width: 18px;
  height: 18px;
  flex: none;
  place-items: center;
  border: 1.5px solid color-mix(in oklab, var(--border) 90%, transparent);
  border-radius: 50%;
  transition:
    background-color 250ms ease,
    border-color 250ms ease;
}
.steps li[data-state='done'] .dot {
  border-color: var(--success);
  background: var(--success);
  color: var(--card);
}
.steps li[data-state='active'] .dot {
  border-color: var(--primary);
}
.pulse {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: var(--primary);
}
.pulse::after {
  content: '';
  position: absolute;
  inset: -1.5px;
  border: 1.5px solid var(--primary);
  border-radius: 50%;
  animation: dot-ping 1.4s cubic-bezier(0, 0, 0.2, 1) infinite;
}

/* ---------- Body ---------- */
.body {
  display: flex;
  min-height: 0;
  flex: 1;
  flex-direction: column;
  gap: 12px;
  margin-top: 16px;
}
.source,
.callout {
  display: flex;
  align-items: center;
  gap: 10px;
  min-height: 56px;
  padding: 12px 14px;
  border-radius: 12px;
  background: var(--surface-soft);
  overflow: hidden;
}
.source-glyph,
.callout-glyph {
  display: grid;
  width: 30px;
  height: 30px;
  flex: none;
  place-items: center;
  border-radius: 50%;
  background: color-mix(in oklab, var(--primary) 12%, transparent);
  color: var(--primary);
}
.source-copy {
  min-width: 0;
  flex: 1;
}
.source-label {
  display: block;
  margin-bottom: 2px;
  color: var(--muted-foreground);
  font-size: 0.6875rem;
  font-weight: 600;
}
.source code {
  display: block;
  min-width: 0;
  color: var(--foreground);
  font-family: ui-monospace, 'Cascadia Code', Consolas, monospace;
  font-size: 0.75rem;
  line-height: 1.3;
}
.source-text,
.callout p {
  margin: 0;
  color: var(--foreground);
  font-size: 0.8125rem;
  line-height: 1.45;
}
.callout {
  align-items: flex-start;
  border: 1px solid color-mix(in oklab, var(--warning) 45%, var(--border));
  background: color-mix(in oklab, var(--warning) 9%, var(--card));
  animation: callout-glow 2s ease-in-out infinite;
}
.callout-glyph {
  background: color-mix(in oklab, var(--warning) 16%, transparent);
  color: var(--warning);
  animation: glyph-bob 1.6s ease-in-out infinite;
}
.callout strong {
  display: block;
  margin-bottom: 3px;
  font-size: 0.8125rem;
}
.callout .pt-btn {
  margin-top: 10px;
}
.pt-btn.small {
  min-height: 32px;
  padding: 0 12px;
  font-size: 0.75rem;
}

/* ---------- Progress bar ---------- */
.bar-row {
  display: flex;
  align-items: center;
  gap: 10px;
}
.bar {
  position: relative;
  height: 6px;
  flex: 1;
  overflow: hidden;
  border-radius: 999px;
  background: color-mix(in oklab, var(--accent) 16%, var(--muted));
}
.fill {
  position: relative;
  display: block;
  height: 100%;
  overflow: hidden;
  border-radius: inherit;
  background: linear-gradient(90deg, color-mix(in oklab, var(--primary) 70%, transparent), var(--primary));
  transition: width 420ms cubic-bezier(0.22, 1, 0.36, 1);
}
/* Moving sheen on the filled part so a slow step still reads as "working". */
.fill::after {
  content: '';
  position: absolute;
  inset: 0;
  background: linear-gradient(
    90deg,
    transparent 0%,
    color-mix(in oklab, white 45%, transparent) 50%,
    transparent 100%
  );
  background-size: 60% 100%;
  background-repeat: no-repeat;
  animation: sheen 1.4s ease-in-out infinite;
}
.sweep {
  position: absolute;
  top: 0;
  bottom: 0;
  left: 0;
  width: 38%;
  border-radius: inherit;
  background: linear-gradient(
    90deg,
    transparent,
    var(--accent) 35%,
    var(--accent) 65%,
    transparent
  );
  animation: sweep 1.35s cubic-bezier(0.45, 0, 0.55, 1) infinite;
}
.bar-pct {
  min-width: 36px;
  color: var(--foreground);
  font-size: 0.75rem;
  font-weight: 700;
  font-variant-numeric: tabular-nums;
  text-align: right;
}

/* ---------- Stats ---------- */
.stats {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: 8px;
}
.stat {
  min-width: 0;
  padding: 10px 12px;
  overflow: hidden;
  border-radius: 12px;
  background: var(--surface-soft);
}
.stat span {
  display: block;
  overflow: hidden;
  color: var(--muted-foreground);
  font-size: 0.6875rem;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.stat strong {
  display: block;
  margin-top: 4px;
  overflow: hidden;
  font-size: 0.9375rem;
  font-weight: 700;
  font-variant-numeric: tabular-nums;
  text-overflow: ellipsis;
  white-space: nowrap;
}

/* ---------- Scan checklist ---------- */
.areas {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 4px 12px;
  list-style: none;
  margin: 0;
  padding: 0;
}
.areas li {
  display: flex;
  align-items: center;
  gap: 8px;
  min-height: 28px;
  padding: 0 8px;
  border-radius: 8px;
  color: var(--muted-foreground);
  font-size: 0.8125rem;
  transition:
    background-color 250ms ease,
    color 250ms ease;
}
.areas li[data-state='done'] {
  color: var(--foreground);
}
.areas li[data-state='done'] svg {
  color: var(--success);
}
.areas li[data-state='active'] {
  background: color-mix(in oklab, var(--primary) 9%, transparent);
  color: var(--foreground);
  font-weight: 650;
}
.areas li[data-state='active'] svg {
  color: var(--primary);
}
.pending-dot {
  width: 14px;
  height: 14px;
  flex: none;
  border: 1.5px solid color-mix(in oklab, var(--border) 90%, transparent);
  border-radius: 50%;
}

/* ---------- Clean log ---------- */
.log {
  position: relative;
  min-height: 120px;
  max-height: 220px;
  flex: 1;
  overflow: auto;
  list-style: none;
  margin: 0;
  padding: 6px;
  border-radius: 12px;
  background: var(--surface-soft);
}
.log li {
  display: flex;
  min-width: 0;
  align-items: center;
  gap: 8px;
  min-height: 26px;
  padding: 0 6px;
  font-size: 0.75rem;
}
.log li .path {
  min-width: 0;
  flex: 1;
  color: var(--muted-foreground);
}
.log li em {
  flex: none;
  font-size: 0.6875rem;
  font-style: normal;
  font-weight: 650;
}
.log li[data-tone='ok'] svg,
.log li[data-tone='ok'] em {
  color: var(--success);
}
.log li[data-tone='warn'] svg,
.log li[data-tone='warn'] em {
  color: var(--warning);
}
.log li[data-tone='bad'] svg,
.log li[data-tone='bad'] em {
  color: var(--destructive);
}
.log-enter-active {
  transition:
    opacity 260ms ease,
    transform 260ms cubic-bezier(0.22, 1, 0.36, 1),
    background-color 900ms ease;
}
.log-enter-from {
  opacity: 0;
  transform: translateY(-6px);
  background: color-mix(in oklab, var(--primary) 14%, transparent);
}
.log-move {
  transition: transform 260ms cubic-bezier(0.22, 1, 0.36, 1);
}
.log-leave-active {
  display: none;
}

.hint {
  margin: 0;
  color: var(--muted-foreground);
  font-size: 0.75rem;
  text-align: center;
}

/* ---------- Footer ---------- */
.foot {
  margin-top: 14px;
}
.cancel {
  width: 100%;
  min-height: 42px;
  border: 1px solid var(--border);
  border-radius: 14px;
  background: var(--surface-soft);
  color: var(--foreground);
  font-size: 0.875rem;
  font-weight: 600;
  cursor: pointer;
}
.cancel:hover {
  background: var(--muted);
}

/* ---------- Motion ---------- */
.spin {
  animation: ring-spin 0.9s linear infinite;
}
.pop {
  animation: pop-in 320ms cubic-bezier(0.34, 1.56, 0.64, 1);
}
.swap-enter-active,
.swap-leave-active {
  transition:
    opacity 180ms ease,
    transform 180ms ease;
}
.swap-enter-from {
  opacity: 0;
  transform: translateY(4px);
}
.swap-leave-to {
  opacity: 0;
}

@keyframes ring-spin {
  to {
    rotate: 360deg;
  }
}
@keyframes ring-breathe {
  0%,
  100% {
    opacity: 0.8;
  }
  50% {
    opacity: 1;
  }
}
@keyframes sweep {
  from {
    transform: translateX(-100%);
  }
  to {
    transform: translateX(265%);
  }
}
@keyframes sheen {
  from {
    background-position: -120% 0;
  }
  to {
    background-position: 220% 0;
  }
}
@keyframes dot-ping {
  0% {
    opacity: 0.8;
    transform: scale(1);
  }
  80%,
  100% {
    opacity: 0;
    transform: scale(1.8);
  }
}
@keyframes pop-in {
  from {
    opacity: 0;
    transform: scale(0.4);
  }
  to {
    opacity: 1;
    transform: scale(1);
  }
}
@keyframes callout-glow {
  0%,
  100% {
    box-shadow: 0 0 0 0 color-mix(in oklab, var(--warning) 0%, transparent);
  }
  50% {
    box-shadow: 0 0 0 4px color-mix(in oklab, var(--warning) 18%, transparent);
  }
}
@keyframes glyph-bob {
  0%,
  100% {
    transform: translateY(0);
  }
  50% {
    transform: translateY(-2px);
  }
}
@keyframes panel-in {
  from {
    opacity: 0;
    transform: translateY(8px) scale(0.98);
  }
  to {
    opacity: 1;
    transform: none;
  }
}
@keyframes fade-in {
  from {
    opacity: 0;
  }
}

@media (max-width: 520px) {
  .step-label {
    display: none;
  }
  .steps li[data-state='active'] .step-label {
    display: inline;
  }
}

@media (prefers-reduced-motion: reduce) {
  .overlay,
  .panel,
  .ring-disc,
  .fill::after,
  .pulse::after,
  .pop,
  .callout,
  .callout-glyph {
    animation: none;
  }
  /* Keep the busy indicators moving — they show work in progress, not decoration. */
  .ring-spinner {
    animation-duration: 1.25s;
  }
  .sweep {
    animation-duration: 2.4s;
  }
}
</style>

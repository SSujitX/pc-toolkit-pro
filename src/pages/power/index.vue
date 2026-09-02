<script setup lang="ts">
import { computed, onMounted, ref } from 'vue';
import { useI18n } from 'vue-i18n';
import { Moon, Power, RotateCcw, Lock, LogOut, Bed, Timer } from '@lucide/vue';
import PtPageShell from '@/components/custom/pt-page-shell.vue';
import PtConfirmDialog from '@/components/custom/pt-confirm-dialog.vue';
import type { PowerAction } from '@/lib/models/actions';
import { usePowerStore } from '@/stores/power-store';

const { t } = useI18n();
const store = usePowerStore();
const amount = ref(30);
const unit = ref<'minutes' | 'hours'>('minutes');
const confirmOpen = ref(false);
const pendingAction = ref<PowerAction | 'schedule' | null>(null);

const actions: { id: PowerAction; labelKey: string; needsConfirm: boolean; icon: unknown }[] = [
  { id: 'shutdown', labelKey: 'power.shutdown', needsConfirm: true, icon: Power },
  { id: 'restart', labelKey: 'power.restart', needsConfirm: true, icon: RotateCcw },
  { id: 'sleep', labelKey: 'power.sleep', needsConfirm: false, icon: Moon },
  { id: 'hibernate', labelKey: 'power.hibernate', needsConfirm: false, icon: Bed },
  { id: 'lock', labelKey: 'power.lock', needsConfirm: false, icon: Lock },
  { id: 'signOut', labelKey: 'power.signOut', needsConfirm: true, icon: LogOut },
];

const confirmMessage = computed(() => {
  if (pendingAction.value === 'schedule') {
    return t('power.confirmSchedule', { value: amount.value, unit: t(`power.${unit.value}`) });
  }
  if (pendingAction.value === 'shutdown') return t('power.confirmShutdown');
  if (pendingAction.value === 'restart') return t('power.confirmRestart');
  if (pendingAction.value === 'signOut') return t('power.confirmSignOut');
  return '';
});

const confirmTitle = computed(() => {
  if (pendingAction.value === 'schedule') return t('power.scheduleTitle');
  return t('power.title');
});

onMounted(() => {
  store.ensureCountdown();
});

function clickAction(action: PowerAction, needsConfirm: boolean) {
  if (!needsConfirm) {
    void store.execute(action);
    return;
  }
  pendingAction.value = action;
  confirmOpen.value = true;
}

function clickSchedule() {
  pendingAction.value = 'schedule';
  confirmOpen.value = true;
}

function onConfirm() {
  if (pendingAction.value === 'schedule') {
    clampAmount();
    const seconds = unit.value === 'hours' ? amount.value * 3600 : amount.value * 60;
    void store.schedule(seconds);
  } else if (pendingAction.value) {
    void store.execute(pendingAction.value);
  }
  pendingAction.value = null;
}

function formatCountdown(total: number) {
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = total % 60;
  if (h > 0) {
    return `${h.toString().padStart(2, '0')}:${m.toString().padStart(2, '0')}:${s
      .toString()
      .padStart(2, '0')}`;
  }
  return `${m.toString().padStart(2, '0')}:${s.toString().padStart(2, '0')}`;
}

function clampAmount() {
  if (!Number.isFinite(amount.value) || amount.value < 1) amount.value = 1;
  if (amount.value > 1440) amount.value = 1440;
}
</script>

<template>
  <PtPageShell :title="t('power.title')" :subtitle="t('power.subtitle')">
    <div class="grid">
      <button
        v-for="action in actions"
        :key="action.id"
        type="button"
        class="card-btn"
        :disabled="store.busy"
        @click="clickAction(action.id, action.needsConfirm)"
      >
        <span class="card-icon" aria-hidden="true">
          <component :is="action.icon" :size="18" :stroke-width="1.9" />
        </span>
        {{ t(action.labelKey) }}
      </button>
    </div>

    <section class="schedule-card" :class="{ active: store.hasActiveSchedule }">
      <header class="schedule-head">
        <div class="schedule-icon" aria-hidden="true">
          <Timer :size="18" :stroke-width="1.9" />
        </div>
        <div class="schedule-copy">
          <h2>
            {{
              store.hasActiveSchedule
                ? t('power.activeTitle')
                : t('power.scheduleTitle')
            }}
          </h2>
          <p>
            {{
              store.hasActiveSchedule
                ? t('power.activeBody')
                : t('power.scheduleBody')
            }}
          </p>
        </div>
      </header>

      <div v-if="store.hasActiveSchedule" class="countdown" role="status" aria-live="polite">
        <div class="countdown-main">
          <small>{{ t('power.activeLabel') }}</small>
          <strong>{{ formatCountdown(store.countdownSeconds) }}</strong>
        </div>
        <button
          type="button"
          class="pt-btn pt-btn-danger cancel-btn"
          :disabled="store.busy"
          @click="store.cancelSchedule()"
        >
          {{ t('power.cancelSchedule') }}
        </button>
      </div>

      <div v-else class="schedule-toolbar">
        <label class="field">
          <span>{{ t('power.delay') }}</span>
          <input
            v-model.number="amount"
            class="amount"
            type="number"
            min="1"
            max="1440"
            @blur="clampAmount"
          />
        </label>

        <div class="unit-toggle" role="group" :aria-label="t('power.delay')">
          <button
            type="button"
            :class="{ active: unit === 'minutes' }"
            @click="unit = 'minutes'"
          >
            {{ t('power.minutes') }}
          </button>
          <button
            type="button"
            :class="{ active: unit === 'hours' }"
            @click="unit = 'hours'"
          >
            {{ t('power.hours') }}
          </button>
        </div>

        <button
          type="button"
          class="pt-btn pt-btn-primary schedule-submit"
          :disabled="store.busy"
          @click="clickSchedule"
        >
          {{ t('power.schedule') }}
        </button>
      </div>
    </section>

    <PtConfirmDialog
      v-model:open="confirmOpen"
      :title="confirmTitle"
      :message="confirmMessage"
      :confirm-text="
        pendingAction === 'schedule' ? t('power.scheduleConfirm') : t('common.confirm')
      "
      destructive
      @confirm="onConfirm"
    />
  </PtPageShell>
</template>

<style scoped>
.grid {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: 10px;
}
.card-btn {
  display: flex;
  align-items: center;
  justify-content: flex-start;
  gap: 12px;
  min-height: 56px;
  border: 1px solid color-mix(in oklab, var(--border) 80%, transparent);
  border-radius: 14px;
  background: var(--card);
  color: var(--foreground);
  padding: 12px 14px;
  font-size: 0.875rem;
  font-weight: 600;
  cursor: pointer;
  box-shadow: var(--shadow-card);
  transition:
    background-color 0.16s ease,
    border-color 0.16s ease;
}
.card-btn:hover:not(:disabled) {
  background: var(--surface-soft);
  border-color: color-mix(in oklab, var(--border) 60%, transparent);
}
.card-btn:disabled {
  opacity: 0.55;
  cursor: not-allowed;
}
.card-icon {
  display: grid;
  width: 34px;
  height: 34px;
  flex: none;
  place-items: center;
  border-radius: 10px;
  background: color-mix(in oklab, var(--primary) 12%, transparent);
  color: var(--primary);
}

.schedule-card {
  display: flex;
  flex-direction: column;
  gap: 14px;
  margin-top: 2px;
  padding: 16px 18px;
  border: 1px solid color-mix(in oklab, var(--border) 80%, transparent);
  border-radius: 14px;
  background: var(--card);
  box-shadow: var(--shadow-card);
}
.schedule-card.active {
  border-color: color-mix(in oklab, var(--warning) 55%, var(--border));
  background: color-mix(in oklab, var(--warning) 6%, var(--card));
}
.schedule-head {
  display: flex;
  align-items: flex-start;
  gap: 12px;
}
.schedule-icon {
  display: grid;
  width: 36px;
  height: 36px;
  flex: none;
  place-items: center;
  border-radius: 10px;
  background: color-mix(in oklab, var(--warning) 14%, transparent);
  color: var(--warning);
}
.schedule-copy h2 {
  margin: 0;
  font-size: 0.9375rem;
  font-weight: 650;
  letter-spacing: -0.02em;
}
.schedule-copy p {
  margin: 3px 0 0;
  color: var(--muted-foreground);
  font-size: 0.75rem;
  line-height: 1.4;
}

.countdown {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 14px;
  padding: 16px 16px;
  border-radius: 14px;
  border: 1px solid color-mix(in oklab, var(--warning) 35%, var(--border));
  background: color-mix(in oklab, var(--warning) 10%, var(--card));
}
.countdown-main {
  min-width: 0;
}
.countdown small {
  display: block;
  color: var(--warning);
  font-size: 0.75rem;
  font-weight: 700;
  letter-spacing: 0.02em;
  text-transform: uppercase;
}
.countdown strong {
  display: block;
  margin-top: 4px;
  font-size: 2rem;
  font-weight: 750;
  letter-spacing: -0.04em;
  font-variant-numeric: tabular-nums;
  color: var(--foreground);
  line-height: 1.1;
}
.cancel-btn {
  flex: none;
  min-height: 44px;
  padding-inline: 16px;
}

.schedule-toolbar {
  display: flex;
  flex-wrap: wrap;
  align-items: flex-end;
  gap: 10px;
}
.field {
  display: flex;
  flex-direction: column;
  gap: 6px;
  min-width: 96px;
}
.field > span {
  color: var(--muted-foreground);
  font-size: 0.6875rem;
  font-weight: 650;
  letter-spacing: 0.01em;
}
.amount {
  box-sizing: border-box;
  width: 96px;
  height: 40px;
  min-height: 40px;
  padding: 0 12px;
  border: 1px solid color-mix(in oklab, var(--border) 85%, transparent);
  border-radius: 12px;
  background: var(--card);
  color: var(--foreground);
  font-size: 0.875rem;
  font-weight: 600;
  font-variant-numeric: tabular-nums;
  outline: none;
}
.amount:focus {
  border-color: color-mix(in oklab, var(--primary) 55%, var(--border));
  box-shadow: 0 0 0 3px color-mix(in oklab, var(--primary) 16%, transparent);
}
.amount::-webkit-outer-spin-button,
.amount::-webkit-inner-spin-button {
  opacity: 0.55;
}

.unit-toggle {
  display: inline-grid;
  grid-template-columns: 1fr 1fr;
  height: 40px;
  min-height: 40px;
  padding: 3px;
  border: 1px solid color-mix(in oklab, var(--border) 85%, transparent);
  border-radius: 12px;
  background: var(--surface-soft);
}
.unit-toggle button {
  min-width: 88px;
  height: 100%;
  border: 0;
  border-radius: 9px;
  background: transparent;
  color: var(--muted-foreground);
  font-size: 0.8125rem;
  font-weight: 600;
  cursor: pointer;
}
.unit-toggle button.active {
  background: var(--card);
  color: var(--foreground);
  box-shadow: 0 1px 2px color-mix(in oklab, var(--foreground) 8%, transparent);
}
.unit-toggle button:hover:not(.active) {
  color: var(--foreground);
}

.schedule-submit {
  margin-inline-start: auto;
}

@media (max-width: 720px) {
  .grid {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }
  .schedule-toolbar {
    align-items: stretch;
  }
  .field {
    flex: 1;
    min-width: 0;
  }
  .amount {
    width: 100%;
  }
  .unit-toggle {
    width: 100%;
  }
  .schedule-submit {
    width: 100%;
    margin-inline-start: 0;
  }
  .countdown {
    flex-direction: column;
    align-items: stretch;
  }
  .cancel-btn {
    width: 100%;
  }
}
</style>

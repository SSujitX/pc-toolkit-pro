<script setup lang="ts">
import { computed, ref } from 'vue';
import { useI18n } from 'vue-i18n';
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

const actions: { id: PowerAction; labelKey: string; needsConfirm: boolean }[] = [
  { id: 'shutdown', labelKey: 'power.shutdown', needsConfirm: true },
  { id: 'restart', labelKey: 'power.restart', needsConfirm: true },
  { id: 'sleep', labelKey: 'power.sleep', needsConfirm: false },
  { id: 'hibernate', labelKey: 'power.hibernate', needsConfirm: false },
  { id: 'lock', labelKey: 'power.lock', needsConfirm: false },
  { id: 'signOut', labelKey: 'power.signOut', needsConfirm: true },
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
  return `${h.toString().padStart(2, '0')}:${m.toString().padStart(2, '0')}:${s
    .toString()
    .padStart(2, '0')}`;
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
        {{ t(action.labelKey) }}
      </button>
    </div>

    <div class="schedule">
      <div v-if="store.deadline" class="countdown">
        {{ formatCountdown(store.countdownSeconds) }}
        <button type="button" class="btn" @click="store.cancelSchedule()">
          {{ t('power.cancelSchedule') }}
        </button>
      </div>
      <div class="schedule-row">
        <input v-model.number="amount" type="number" min="1" max="1440" />
        <select v-model="unit">
          <option value="minutes">{{ t('power.minutes') }}</option>
          <option value="hours">{{ t('power.hours') }}</option>
        </select>
        <button type="button" class="btn primary" @click="clickSchedule">
          {{ t('power.schedule') }}
        </button>
      </div>
    </div>

    <PtConfirmDialog
      v-model:open="confirmOpen"
      :title="t('power.title')"
      :message="confirmMessage"
      destructive
      @confirm="onConfirm"
    />
  </PtPageShell>
</template>

<style scoped>
.grid {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: 8px;
}
.card-btn,
.btn {
  border: 1px solid var(--border);
  border-radius: 8px;
  background: var(--card);
  color: var(--foreground);
  padding: 14px 12px;
  font-size: 0.8125rem;
  font-weight: 600;
  cursor: pointer;
}
.card-btn:hover,
.btn:hover {
  background: var(--muted);
}
.btn.primary {
  background: var(--primary);
  border-color: var(--primary);
  color: var(--primary-foreground);
}
.schedule {
  margin-top: 8px;
  display: flex;
  flex-direction: column;
  gap: 10px;
}
.schedule-row {
  display: flex;
  gap: 8px;
  align-items: center;
}
.schedule-row input,
.schedule-row select {
  border: 1px solid var(--border);
  border-radius: 8px;
  background: var(--card);
  color: var(--foreground);
  padding: 8px 10px;
}
.countdown {
  display: flex;
  align-items: center;
  gap: 12px;
  font-size: 1.25rem;
  font-weight: 650;
}
</style>

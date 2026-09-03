<script setup lang="ts">
import { computed, ref, watch } from 'vue';
import { useI18n } from 'vue-i18n';
import { AppWindow, X } from '@lucide/vue';
import { formatBytes } from '@/lib/utils/format';
import {
  closeAppGroupsForRules,
  type DeepCleanupCloseAppGroup,
  type DeepCleanupRule,
} from '@/lib/models/deep-cleaner';
import { DeepCleanerService } from '@/lib/services/api-services';

const open = defineModel<boolean>('open', { default: false });

const props = defineProps<{
  rules: DeepCleanupRule[];
  selectedBytes: number;
}>();

const emit = defineEmits<{ confirm: []; cancel: [] }>();
const { t } = useI18n();

const phase = ref<'plan' | 'force'>('plan');
const selectedCloseIds = ref<string[]>([]);
const closeGroups = ref<DeepCleanupCloseAppGroup[]>([]);
const remainingGroups = ref<DeepCleanupCloseAppGroup[]>([]);
const probing = ref(false);
const closing = ref(false);

const planItems = computed(() =>
  [...props.rules].sort((a, b) => b.bytes - a.bytes).map((rule) => ({
    id: rule.id,
    name: t(rule.nameKey),
    detail: t(rule.detailKey),
    bytes: rule.bytes,
  }))
);

const selectedCloseGroups = computed(() => {
  const selected = new Set(selectedCloseIds.value);
  return closeGroups.value.filter((group) => selected.has(group.id));
});

const closeNames = computed(() => [
  ...new Set(selectedCloseGroups.value.flatMap((group) => group.processes)),
]);

const primaryLabel = computed(() => {
  if (closing.value) return t('deepCleaner.confirm.closingApps');
  if (phase.value === 'force') return t('deepCleaner.confirm.forceAndContinue');
  if (selectedCloseIds.value.length) {
    return t('deepCleaner.confirm.closeAndContinue', { count: selectedCloseIds.value.length });
  }
  return t('deepCleaner.confirm.startCleanup');
});

watch(
  () => open.value,
  async (isOpen) => {
    if (!isOpen) return;
    phase.value = 'plan';
    selectedCloseIds.value = [];
    remainingGroups.value = [];
    closeGroups.value = [];
    probing.value = true;
    try {
      const names = [
        ...new Set(props.rules.flatMap((rule) => rule.relatedProcesses ?? [])),
      ];
      const running = names.length
        ? await DeepCleanerService.probeRunningProcesses(names)
        : [];
      closeGroups.value = closeAppGroupsForRules(props.rules, running);
      selectedCloseIds.value = closeGroups.value.map((group) => group.id);
    } catch {
      closeGroups.value = [];
    } finally {
      probing.value = false;
    }
  }
);

function toggleCloseGroup(id: string) {
  if (selectedCloseIds.value.includes(id)) {
    selectedCloseIds.value = selectedCloseIds.value.filter((item) => item !== id);
  } else {
    selectedCloseIds.value = [...selectedCloseIds.value, id];
  }
}

function selectAllCloseGroups() {
  selectedCloseIds.value = closeGroups.value.map((group) => group.id);
}

function onCancel() {
  open.value = false;
  emit('cancel');
}

async function onPrimary() {
  if (closing.value || probing.value) return;

  if (phase.value === 'force') {
    closing.value = true;
    try {
      const names = [...new Set(remainingGroups.value.flatMap((g) => g.processes))];
      await DeepCleanerService.closeRunningProcesses(names, true);
    } catch {
      /* continue anyway — cleanup still skip-and-continue */
    } finally {
      closing.value = false;
    }
    open.value = false;
    emit('confirm');
    return;
  }

  if (!closeNames.value.length) {
    open.value = false;
    emit('confirm');
    return;
  }

  closing.value = true;
  try {
    const result = await DeepCleanerService.closeRunningProcesses(closeNames.value, false);
    const remaining = new Set(
      result.targets
        .filter((target) => target.remainingCount > 0 || target.status !== 'closed')
        .map((target) => target.imageName.toLowerCase())
    );
    if (!remaining.size) {
      open.value = false;
      emit('confirm');
      return;
    }
    remainingGroups.value = selectedCloseGroups.value
      .map((group) => ({
        ...group,
        processes: group.processes.filter((name) => remaining.has(name.toLowerCase())),
      }))
      .filter((group) => group.processes.length > 0);
    phase.value = 'force';
  } catch {
    open.value = false;
    emit('confirm');
  } finally {
    closing.value = false;
  }
}

function skipForce() {
  open.value = false;
  emit('confirm');
}
</script>

<template>
  <div v-if="open" class="confirm-root" role="dialog" aria-modal="true">
    <div class="confirm-overlay" @click="onCancel" />
    <div class="confirm-panel">
      <header class="confirm-head">
        <div>
          <h2>{{ t('deepCleaner.confirm.title') }}</h2>
          <p class="summary">
            <span>{{ t('deepCleaner.confirm.selectedCount', { count: rules.length }) }}</span>
            <span aria-hidden="true">·</span>
            <span>{{ t('deepCleaner.confirm.estimated') }}</span>
            <strong>{{ formatBytes(selectedBytes) }}</strong>
          </p>
        </div>
        <button type="button" class="icon-btn" :aria-label="t('common.cancel')" @click="onCancel">
          <X :size="16" :stroke-width="2" />
        </button>
      </header>

      <div class="confirm-body">
        <p v-if="closeGroups.length && phase === 'plan'" class="warning">
          {{ t('deepCleaner.confirm.closeAppsHint') }}
        </p>

        <section v-if="phase === 'plan' && closeGroups.length" class="close-panel">
          <header>
            <strong>{{ t('deepCleaner.confirm.closeAppsTitle') }}</strong>
            <button type="button" class="text-btn" @click="selectAllCloseGroups">
              {{ t('deepCleaner.selectAll') }}
            </button>
          </header>
          <ul>
            <li v-for="group in closeGroups" :key="group.id">
              <label>
                <input
                  type="checkbox"
                  :checked="selectedCloseIds.includes(group.id)"
                  @change="toggleCloseGroup(group.id)"
                />
                <span class="app-icon" aria-hidden="true">
                  <AppWindow :size="15" :stroke-width="1.8" />
                </span>
                <span class="app-copy">
                  <strong>{{ t(group.name) }}</strong>
                  <small>
                    {{
                      t('deepCleaner.confirm.relatedProcess', {
                        count: group.processCount,
                        name: group.processes.join(', '),
                      })
                    }}
                  </small>
                </span>
              </label>
            </li>
          </ul>
        </section>

        <section v-else-if="phase === 'force'" class="close-panel force">
          <p class="force-warn">
            <strong>{{ t('deepCleaner.confirm.forceTitle') }}</strong>
            <span>{{ t('deepCleaner.confirm.forceBody') }}</span>
          </p>
          <ul>
            <li v-for="group in remainingGroups" :key="group.id">
              <span class="app-icon" aria-hidden="true">
                <AppWindow :size="15" :stroke-width="1.8" />
              </span>
              <span class="app-copy">
                <strong>{{ t(group.name) }}</strong>
                <small>{{ group.processes.join(', ') }}</small>
              </span>
            </li>
          </ul>
        </section>

        <section class="plan-list">
          <div v-for="item in planItems" :key="item.id" class="plan-row">
            <span class="plan-copy">
              <strong>{{ item.name }}</strong>
              <small :title="item.detail">{{ item.detail }}</small>
            </span>
            <strong class="plan-size">{{ formatBytes(item.bytes) }}</strong>
          </div>
        </section>
      </div>

      <footer class="confirm-foot">
        <button
          v-if="phase === 'force'"
          type="button"
          class="pt-btn"
          :disabled="closing"
          @click="skipForce"
        >
          {{ t('deepCleaner.confirm.skipAndContinue') }}
        </button>
        <button v-else type="button" class="pt-btn" :disabled="closing" @click="onCancel">
          {{ t('deepCleaner.confirm.back') }}
        </button>
        <button
          type="button"
          class="pt-btn pt-btn-primary"
          :class="{ 'pt-btn-danger': phase === 'force' }"
          :disabled="closing || probing"
          @click="onPrimary"
        >
          {{ primaryLabel }}
        </button>
      </footer>
    </div>
  </div>
</template>

<style scoped>
.confirm-root {
  position: fixed;
  inset: 0;
  z-index: 80;
  display: grid;
  place-items: center;
  padding: 16px;
}
.confirm-overlay {
  position: absolute;
  inset: 0;
  background: rgb(0 0 0 / 32%);
}
.confirm-panel {
  position: relative;
  z-index: 1;
  display: flex;
  width: min(640px, calc(100vw - 32px));
  max-height: min(820px, calc(100vh - 32px));
  flex-direction: column;
  border: 1px solid var(--border);
  border-radius: 14px;
  background: var(--card);
  box-shadow: 0 25px 50px -12px rgb(0 0 0 / 28%);
}
.confirm-head {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 12px;
  padding: 16px 18px 10px;
}
.confirm-head h2 {
  margin: 0;
  font-size: 1.05rem;
  font-weight: 700;
}
.summary {
  display: flex;
  flex-wrap: wrap;
  align-items: baseline;
  gap: 6px;
  margin: 6px 0 0;
  color: var(--muted-foreground);
  font-size: 0.8125rem;
}
.summary strong {
  color: var(--primary);
  font-size: 1.05rem;
  font-weight: 700;
}
.icon-btn {
  display: grid;
  width: 32px;
  height: 32px;
  flex: none;
  place-items: center;
  border: 1px solid transparent;
  border-radius: 8px;
  background: transparent;
  color: var(--muted-foreground);
  cursor: pointer;
}
.icon-btn:hover {
  background: var(--muted);
  color: var(--foreground);
}
.confirm-body {
  min-height: 0;
  flex: 1;
  overflow: auto;
  padding: 0 18px 8px;
}
.warning {
  margin: 0 0 10px;
  border-radius: 8px;
  background: color-mix(in oklab, var(--warning, #d97706) 14%, var(--card));
  color: var(--foreground);
  padding: 8px 10px;
  font-size: 0.75rem;
  line-height: 1.4;
}
.close-panel {
  margin-bottom: 10px;
  border: 1px solid color-mix(in oklab, var(--border) 80%, transparent);
  border-radius: 10px;
  background: var(--surface-soft);
  overflow: hidden;
}
.close-panel header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
  padding: 10px 12px;
  border-bottom: 1px solid color-mix(in oklab, var(--border) 70%, transparent);
}
.close-panel header strong {
  font-size: 0.8125rem;
}
.text-btn {
  border: 1px solid var(--border);
  border-radius: 8px;
  background: var(--card);
  color: var(--primary);
  padding: 4px 8px;
  font-size: 0.6875rem;
  font-weight: 650;
  cursor: pointer;
}
.close-panel ul {
  list-style: none;
  margin: 0;
  padding: 0;
}
.close-panel li {
  display: flex;
  align-items: center;
  gap: 10px;
  min-height: 48px;
  padding: 8px 12px;
  border-top: 1px solid color-mix(in oklab, var(--border) 65%, transparent);
}
.close-panel li:first-child {
  border-top: 0;
}
.close-panel label {
  display: flex;
  align-items: center;
  gap: 10px;
  width: 100%;
  cursor: pointer;
}
.app-icon {
  display: grid;
  width: 28px;
  height: 28px;
  flex: none;
  place-items: center;
  border-radius: 8px;
  background: color-mix(in oklab, var(--primary) 12%, transparent);
  color: var(--primary);
}
.app-copy {
  min-width: 0;
  flex: 1;
}
.app-copy strong {
  display: block;
  font-size: 0.8125rem;
}
.app-copy small {
  display: block;
  margin-top: 2px;
  color: var(--muted-foreground);
  font-size: 0.6875rem;
}
.force-warn {
  display: flex;
  flex-direction: column;
  gap: 3px;
  margin: 0;
  padding: 10px 12px;
  background: color-mix(in oklab, var(--destructive, #dc2626) 10%, var(--card));
  color: var(--foreground);
  font-size: 0.75rem;
}
.plan-list {
  border: 1px solid color-mix(in oklab, var(--border) 80%, transparent);
  border-radius: 10px;
  overflow: hidden;
}
.plan-row {
  display: grid;
  grid-template-columns: minmax(0, 1fr) auto;
  align-items: center;
  gap: 14px;
  min-height: 52px;
  padding: 8px 12px;
  border-top: 1px solid color-mix(in oklab, var(--border) 65%, transparent);
}
.plan-row:first-child {
  border-top: 0;
}
.plan-copy {
  min-width: 0;
}
.plan-copy strong {
  display: block;
  font-size: 0.8125rem;
  font-weight: 600;
}
.plan-copy small {
  display: block;
  overflow: hidden;
  margin-top: 2px;
  color: var(--muted-foreground);
  font-size: 0.6875rem;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.plan-size {
  font-size: 0.8125rem;
  font-weight: 650;
  font-variant-numeric: tabular-nums;
  white-space: nowrap;
}
.confirm-foot {
  display: flex;
  justify-content: flex-end;
  gap: 10px;
  padding: 12px 18px 16px;
  border-top: 1px solid color-mix(in oklab, var(--border) 70%, transparent);
}
</style>

<script setup lang="ts">
import { computed } from 'vue';
import { useI18n } from 'vue-i18n';
import { CircleAlert, CircleCheck, Recycle, X } from '@lucide/vue';
import { formatBytes } from '@/lib/utils/format';
import {
  execReasonKey,
  ruleTitle,
  summarizeExecResults,
  type DeepCleanupRule,
  type DeepCleanupResult,
  type DeepCleanupRuleExecResult,
} from '@/lib/models/deep-cleaner';

const props = defineProps<{
  result: DeepCleanupResult;
  rules: DeepCleanupRule[];
}>();

const emit = defineEmits<{ dismiss: [] }>();
const { t } = useI18n();

const summary = computed(() => summarizeExecResults(props.result.rules));
const hasWarnings = computed(
  () => props.result.cancelled || props.result.rules.some((r) => r.status !== 'completed')
);

const rows = computed(() => {
  const byId = new Map(props.rules.map((rule) => [rule.id, rule] as const));
  return [...props.result.rules]
    .sort((a, b) => b.freedBytes - a.freedBytes)
    .map((item) => {
      const rule = byId.get(item.id);
      return {
        item,
        name: rule ? ruleTitle(rule, t) : item.id,
        reason: reasonLabel(item),
      };
    });
});

function reasonLabel(item: DeepCleanupRuleExecResult): string {
  if (!item.reason) return '';
  if (item.reason === 'itemsBusy') {
    return t('deepCleaner.execReason.itemsBusy', { count: item.busyCount });
  }
  if (item.reason === 'itemsFailed') {
    return t('deepCleaner.execReason.itemsFailed', { count: item.failedCount });
  }
  return t(execReasonKey(item.reason));
}
</script>

<template>
  <section class="result-panel" :class="{ warn: hasWarnings }">
    <header class="result-head">
      <span class="result-icon" aria-hidden="true">
        <Recycle v-if="!hasWarnings" :size="16" :stroke-width="1.75" />
        <CircleAlert v-else :size="16" :stroke-width="1.75" />
      </span>
      <div class="result-copy">
        <strong>
          {{
            result.cancelled
              ? t('deepCleaner.resultPanel.cancelledTitle')
              : t('deepCleaner.resultPanel.title')
          }}
        </strong>
        <span>
          {{
            t('deepCleaner.resultPanel.freed', {
              bytes: formatBytes(result.freedBytes),
              files: result.filesRemoved,
            })
          }}
        </span>
        <small>
          {{
            t('deepCleaner.resultPanel.counts', {
              completed: summary.completed,
              partial: summary.partial,
              failed: summary.failed,
              skipped: summary.skipped + summary.blocked,
            })
          }}
        </small>
      </div>
      <button
        type="button"
        class="icon-btn"
        :aria-label="t('deepCleaner.resultPanel.dismiss')"
        @click="emit('dismiss')"
      >
        <X :size="15" :stroke-width="2" />
      </button>
    </header>

    <p v-if="result.busyCount > 0" class="result-hint">
      {{ t('deepCleaner.resultPanel.busyHint') }}
    </p>

    <ul class="result-list">
      <li v-for="row in rows" :key="row.item.id" class="result-row">
        <span class="status-icon" :class="row.item.status" aria-hidden="true">
          <CircleCheck v-if="row.item.status === 'completed'" :size="14" :stroke-width="2" />
          <CircleAlert v-else :size="14" :stroke-width="2" />
        </span>
        <span class="row-copy">
          <strong>{{ row.name }}</strong>
          <small>
            {{ t(`deepCleaner.execStatus.${row.item.status}`) }}
            <template v-if="row.reason"> · {{ row.reason }}</template>
          </small>
        </span>
        <span class="row-size">
          {{
            t(
              'deepCleaner.resultPanel.ruleFreed',
              { bytes: formatBytes(row.item.freedBytes), count: row.item.removedCount },
              row.item.removedCount
            )
          }}
        </span>
      </li>
    </ul>
  </section>
</template>

<style scoped>
.result-panel {
  display: flex;
  flex-direction: column;
  max-height: 260px;
  border: 1px solid color-mix(in oklab, var(--border) 75%, transparent);
  border-radius: 14px;
  background: var(--card);
  box-shadow: var(--shadow-card);
  overflow: hidden;
}
.result-head {
  display: flex;
  align-items: flex-start;
  gap: 10px;
  padding: 12px 14px;
  background: color-mix(in oklab, var(--primary) 8%, var(--card));
}
.result-panel.warn .result-head {
  background: color-mix(in oklab, var(--warning, #d97706) 10%, var(--card));
}
.result-icon {
  display: grid;
  width: 28px;
  height: 28px;
  flex: none;
  place-items: center;
  border-radius: 8px;
  background: color-mix(in oklab, var(--primary) 14%, transparent);
  color: var(--primary);
}
.result-panel.warn .result-icon {
  background: color-mix(in oklab, var(--warning, #d97706) 18%, transparent);
  color: color-mix(in oklab, var(--warning, #b45309) 85%, var(--foreground));
}
.result-copy {
  display: flex;
  min-width: 0;
  flex: 1;
  flex-direction: column;
  gap: 2px;
}
.result-copy strong {
  font-size: 0.8125rem;
}
.result-copy span {
  font-size: 0.8125rem;
  font-weight: 650;
}
.result-copy small {
  color: var(--muted-foreground);
  font-size: 0.6875rem;
}
.icon-btn {
  display: grid;
  width: 28px;
  height: 28px;
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
.result-hint {
  margin: 0;
  padding: 8px 14px;
  border-top: 1px solid color-mix(in oklab, var(--border) 65%, transparent);
  color: var(--muted-foreground);
  font-size: 0.75rem;
}
.result-list {
  min-height: 0;
  flex: 1;
  margin: 0;
  padding: 0;
  list-style: none;
  overflow: auto;
}
.result-row {
  display: grid;
  grid-template-columns: auto minmax(0, 1fr) auto;
  align-items: center;
  gap: 10px;
  min-height: 44px;
  padding: 6px 14px;
  border-top: 1px solid color-mix(in oklab, var(--border) 65%, transparent);
}
.status-icon {
  display: grid;
  place-items: center;
  color: color-mix(in oklab, var(--warning, #b45309) 85%, var(--foreground));
}
.status-icon.completed {
  color: color-mix(in oklab, var(--success, #15803d) 80%, var(--foreground));
}
.status-icon.failed {
  color: var(--destructive, #dc2626);
}
.row-copy {
  min-width: 0;
}
.row-copy strong {
  display: block;
  overflow: hidden;
  font-size: 0.8125rem;
  font-weight: 600;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.row-copy small {
  color: var(--muted-foreground);
  font-size: 0.6875rem;
}
.row-size {
  color: var(--muted-foreground);
  font-size: 0.75rem;
  font-variant-numeric: tabular-nums;
  white-space: nowrap;
}
</style>

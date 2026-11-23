<script setup lang="ts">
import { computed } from 'vue';
import { useI18n } from 'vue-i18n';
import { AlertTriangle, CheckCircle2, Info, XCircle } from '@lucide/vue';
import { formatBytes } from '@/lib/utils/format';
import type { LeftoverCleanResult, UninstallReport, UninstallStatus } from '@/lib/models/uninstaller';

const props = defineProps<{
  reports: UninstallReport[];
  cleanResult: LeftoverCleanResult | null;
  hadLeftovers: boolean;
  /** Programs whose leftovers were scanned without running an uninstaller (force / removed elsewhere). */
  scannedPrograms: Array<{ id: string; name: string }>;
}>();

const emit = defineEmits<{ done: []; openBackups: [] }>();
const { t } = useI18n();

function tone(status: UninstallStatus): 'ok' | 'warn' | 'bad' {
  if (status === 'completed') return 'ok';
  if (status === 'completedRebootRequired' || status === 'cancelled') return 'warn';
  return 'bad';
}

const leftoverLine = computed(() => {
  if (props.cleanResult) {
    return t(
      'uninstaller.summary.cleanResult',
      {
        removed: props.cleanResult.removedCount,
        bytes: formatBytes(props.cleanResult.freedBytes),
      },
      props.cleanResult.removedCount
    );
  }
  return props.hadLeftovers ? t('uninstaller.summary.leftoversKept') : t('uninstaller.summary.noLeftovers');
});
</script>

<template>
  <div class="summary-wrap">
    <section class="card">
      <h2>{{ t('uninstaller.summary.title') }}</h2>

      <ul v-if="reports.length" class="reports">
        <li v-for="report in reports" :key="report.programId" class="report" :data-tone="tone(report.status)">
          <CheckCircle2 v-if="tone(report.status) === 'ok'" :size="18" :stroke-width="1.9" aria-hidden="true" />
          <AlertTriangle v-else-if="tone(report.status) === 'warn'" :size="18" :stroke-width="1.9" aria-hidden="true" />
          <XCircle v-else :size="18" :stroke-width="1.9" aria-hidden="true" />
          <div>
            <strong>{{ report.programName }}</strong>
            <span>{{ t(`uninstaller.summary.status.${report.status}`) }}</span>
            <small v-if="report.restorePoint !== 'notRequested'">
              {{ t(`uninstaller.summary.restorePoint.${report.restorePoint}`) }}
            </small>
          </div>
        </li>
      </ul>

      <ul v-if="!reports.length && scannedPrograms.length" class="reports">
        <li v-for="program in scannedPrograms" :key="program.id" class="report" :data-tone="cleanResult ? 'ok' : 'warn'">
          <CheckCircle2 v-if="cleanResult" :size="18" :stroke-width="1.9" aria-hidden="true" />
          <AlertTriangle v-else :size="18" :stroke-width="1.9" aria-hidden="true" />
          <div>
            <strong>{{ program.name }}</strong>
            <span>{{ cleanResult ? t('uninstaller.summary.leftoversCleaned') : t('uninstaller.summary.leftoversKept') }}</span>
          </div>
        </li>
      </ul>

      <div class="leftovers">
        <p class="line"><Info :size="15" :stroke-width="1.9" aria-hidden="true" /> {{ leftoverLine }}</p>
        <template v-if="cleanResult">
          <p v-if="cleanResult.pendingRebootCount" class="line warn">
            {{ t('uninstaller.summary.pendingReboot', { count: cleanResult.pendingRebootCount }, cleanResult.pendingRebootCount) }}
          </p>
          <p v-if="cleanResult.skippedNeedAdminCount" class="line warn">
            {{ t('uninstaller.summary.skippedAdmin', { count: cleanResult.skippedNeedAdminCount }, cleanResult.skippedNeedAdminCount) }}
          </p>
          <p v-if="cleanResult.failedCount" class="line bad">
            {{ t('uninstaller.summary.failed', { count: cleanResult.failedCount }, cleanResult.failedCount) }}
          </p>
          <p v-if="cleanResult.registryBackupSaved" class="line">
            {{ t('uninstaller.summary.backupSaved') }}
            <button type="button" class="text-btn" @click="emit('openBackups')">
              {{ t('uninstaller.summary.openBackups') }}
            </button>
          </p>
        </template>
      </div>

      <footer>
        <button type="button" class="pt-btn pt-btn-primary" @click="emit('done')">
          {{ t('uninstaller.summary.done') }}
        </button>
      </footer>
    </section>
  </div>
</template>

<style scoped>
.summary-wrap {
  display: grid;
  flex: 1;
  min-height: 0;
  place-items: center;
  padding: 16px;
}
.card {
  width: min(560px, 100%);
  border: 1px solid color-mix(in oklab, var(--border) 75%, transparent);
  border-radius: 16px;
  background: var(--card);
  box-shadow: var(--shadow-card);
  padding: 20px 22px;
}
h2 {
  margin: 0 0 12px;
  font-size: 1.05rem;
  font-weight: 700;
}
.reports {
  display: flex;
  flex-direction: column;
  gap: 4px;
  list-style: none;
  margin: 0 0 12px;
  padding: 0;
}
.report {
  display: flex;
  align-items: flex-start;
  gap: 10px;
  padding: 8px 10px;
  border-radius: 10px;
  background: var(--surface-soft);
}
.report svg {
  flex: none;
  margin-top: 1px;
}
.report[data-tone='ok'] svg {
  color: var(--success);
}
.report[data-tone='warn'] svg {
  color: var(--warning);
}
.report[data-tone='bad'] svg {
  color: var(--destructive);
}
.report div {
  display: flex;
  min-width: 0;
  flex-direction: column;
  gap: 2px;
}
.report strong {
  font-size: 0.8125rem;
}
.report span,
.report small {
  color: var(--muted-foreground);
  font-size: 0.75rem;
}
.leftovers {
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.line {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 6px;
  margin: 0;
  color: var(--foreground);
  font-size: 0.8125rem;
}
.line svg {
  color: var(--primary);
}
.line.warn {
  color: var(--warning);
}
.line.bad {
  color: var(--destructive);
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
footer {
  display: flex;
  justify-content: flex-end;
  margin-top: 18px;
}
</style>

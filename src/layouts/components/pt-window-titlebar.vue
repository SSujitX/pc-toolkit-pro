<script setup lang="ts">
import { computed } from 'vue';
import { useI18n } from 'vue-i18n';
import { Clock3, HardDrive, Minus, Square, X } from '@lucide/vue';
import { ApplicationWindowService } from '@/lib/services/application-window-service';
import { formatBytes, formatUptime } from '@/lib/utils/format';
import { displayedMemoryPercent } from '@/lib/models/memory-cleaner';
import { useMemoryCleanerStore } from '@/stores/memory-cleaner-store';
import { useMonitorStore } from '@/stores/monitor-store';

const { t } = useI18n();
const monitor = useMonitorStore();
const memory = useMemoryCleanerStore();

const uptimeLabel = computed(() => formatUptime(monitor.snapshot?.uptimeSeconds ?? 0));
const diskFree = computed(() =>
  monitor.snapshot?.diskTotal
    ? monitor.snapshot.diskTotal - (monitor.snapshot.diskUsed ?? 0)
    : 0
);
const diskPercent = computed(() => monitor.snapshot?.diskPercent ?? 0);
// Same GetPerformanceInfo physical load as the Memory page / tray / WMC / IObit.
const memoryPercent = computed(() =>
  displayedMemoryPercent(memory.stats, monitor.snapshot?.memoryPercent)
);
const memoryPercentLabel = computed(() => Math.round(memoryPercent.value));
const optimizing = computed(() => memory.loading);

async function minimize() {
  await ApplicationWindowService.minimize();
}
async function toggleMax() {
  await ApplicationWindowService.toggleMaximize();
}
async function close() {
  await ApplicationWindowService.closeOrHide();
}

/** Same path as tray Clean Memory — uses current Memory Cleaner area settings. */
async function optimizeFromGauge() {
  if (optimizing.value) return;
  await memory.requestOptimize('tray');
}
</script>

<template>
  <div class="titlebar" data-tauri-drag-region>
    <div class="chrome" data-tauri-drag-region="false">
      <div class="status-chip" :title="t('titlebar.uptimeHint')">
        <Clock3 :size="14" :stroke-width="1.75" aria-hidden="true" />
        <div class="status-body">
          <div class="status-text">
            <span class="status-title">{{ t('titlebar.uptime') }}</span>
            <span class="status-value">{{ uptimeLabel }}</span>
          </div>
          <div class="meter meter-hidden" aria-hidden="true">
            <span style="width: 0%" />
          </div>
        </div>
      </div>

      <div class="status-chip disk" :title="t('titlebar.systemDisk')">
        <HardDrive :size="14" :stroke-width="1.75" aria-hidden="true" />
        <div class="status-body">
          <div class="status-text">
            <span class="status-title">{{ t('titlebar.systemDisk') }}</span>
            <span class="status-value">{{ t('titlebar.diskAvailable', { size: formatBytes(diskFree) }) }}</span>
          </div>
          <div class="meter" aria-hidden="true">
            <span :style="{ width: `${Math.min(100, Math.max(0, diskPercent))}%` }" />
          </div>
        </div>
      </div>

      <button
        type="button"
        class="memory-circle"
        :class="{ busy: optimizing }"
        :title="
          optimizing
            ? t('memoryCleaner.optimizing')
            : t('titlebar.optimizeMemory', { percent: memoryPercentLabel })
        "
        :style="{ '--mem': memoryPercent }"
        :disabled="optimizing"
        :aria-busy="optimizing"
        :aria-valuenow="memoryPercentLabel"
        aria-valuemin="0"
        aria-valuemax="100"
        :aria-label="t('titlebar.optimizeMemory', { percent: memoryPercentLabel })"
        @click.stop="optimizeFromGauge"
      >
        <span class="memory-circle-value">{{ memoryPercentLabel }}%</span>
      </button>

      <div class="controls">
        <button type="button" class="ctrl" :aria-label="t('titlebar.minimize')" @click.stop="minimize">
          <Minus :size="16" :stroke-width="2" />
        </button>
        <button type="button" class="ctrl" :aria-label="t('titlebar.maximize')" @click.stop="toggleMax">
          <Square :size="14" :stroke-width="2" />
        </button>
        <button type="button" class="ctrl close" :aria-label="t('titlebar.close')" @click.stop="close">
          <X :size="16" :stroke-width="2" />
        </button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.titlebar {
  position: absolute;
  inset: 0 0 auto 0;
  z-index: 40;
  height: var(--layout-titlebar-height);
  display: flex;
  justify-content: flex-end;
  align-items: flex-start;
  padding: 8px 4px 0 10px;
}
.chrome {
  display: flex;
  align-items: center;
  gap: 10px;
  height: auto;
  margin-right: 18px;
  padding-inline-end: 0;
}
.status-chip {
  display: flex;
  align-items: center;
  gap: 10px;
  box-sizing: border-box;
  width: 210px;
  height: 40px;
  min-height: 40px;
  max-height: 40px;
  padding: 6px 12px;
  border: 1px solid color-mix(in oklab, var(--border) 90%, transparent);
  border-radius: 12px;
  background: var(--card);
  color: var(--muted-foreground);
  box-shadow: 0 1px 0 color-mix(in oklab, var(--foreground) 4%, transparent);
}
.status-chip.disk {
  width: 210px;
}
.status-body {
  display: flex;
  min-width: 0;
  flex: 1;
  flex-direction: column;
  gap: 4px;
  justify-content: center;
}
.status-text {
  display: flex;
  min-width: 0;
  flex-direction: column;
  gap: 1px;
  line-height: 1.15;
}
.status-title {
  font-size: 10px;
  font-weight: 650;
  letter-spacing: 0.01em;
  color: var(--muted-foreground);
  white-space: nowrap;
}
.status-value {
  overflow: hidden;
  color: var(--foreground);
  font-size: 12px;
  font-weight: 600;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.meter {
  width: 100%;
  height: 3px;
  overflow: hidden;
  border-radius: 999px;
  background: color-mix(in oklab, var(--muted) 80%, transparent);
  flex: none;
}
.meter-hidden {
  visibility: hidden;
}
.meter > span {
  display: block;
  height: 100%;
  border-radius: inherit;
  background: color-mix(in oklab, var(--primary) 82%, var(--warning));
}
/* Registered so the ring animates smoothly to the new percentage after an optimize. */
@property --mem {
  syntax: '<number>';
  inherits: true;
  initial-value: 0;
}
.memory-circle {
  --mem: 0;
  position: relative;
  transition: --mem 900ms cubic-bezier(0.22, 1, 0.36, 1);
  box-sizing: border-box;
  display: grid;
  flex: none;
  width: 40px;
  height: 40px;
  min-width: 40px;
  min-height: 40px;
  max-width: 40px;
  max-height: 40px;
  place-items: center;
  aspect-ratio: 1 / 1;
  padding: 0;
  border-radius: 50%;
  border: 1px solid color-mix(in oklab, var(--border) 90%, transparent);
  background:
    radial-gradient(
      circle at center,
      var(--card) 0 62%,
      transparent 63%
    ),
    conic-gradient(
      from -90deg,
      var(--primary) calc(var(--mem) * 1%),
      color-mix(in oklab, var(--muted) 85%, transparent) 0
    );
  box-shadow: 0 1px 0 color-mix(in oklab, var(--foreground) 4%, transparent);
  color: var(--foreground);
  cursor: pointer;
}
.memory-circle:hover:not(:disabled) {
  border-color: color-mix(in oklab, var(--primary) 45%, var(--border));
}
.memory-circle.busy {
  cursor: progress;
}
/* Rotating arc while optimizing (circular, continuous — the shared busy-spinner style). */
.memory-circle.busy::after {
  content: '';
  position: absolute;
  inset: -4px;
  border-radius: 50%;
  background: conic-gradient(
    from 0deg,
    var(--primary) 0deg,
    color-mix(in oklab, var(--primary) 45%, transparent) 70deg,
    transparent 120deg
  );
  -webkit-mask: radial-gradient(farthest-side, transparent calc(100% - 2.5px), #000 calc(100% - 2px));
  mask: radial-gradient(farthest-side, transparent calc(100% - 2.5px), #000 calc(100% - 2px));
  animation: mem-spin 0.8s linear infinite;
  pointer-events: none;
}
@keyframes mem-spin {
  to {
    rotate: 360deg;
  }
}
@media (prefers-reduced-motion: reduce) {
  .memory-circle {
    transition: none;
  }
  /* Busy stays visible as a softly pulsing ring instead of a spinning arc. */
  .memory-circle.busy::after {
    background: color-mix(in oklab, var(--primary) 70%, transparent);
    animation: mem-pulse 1.6s ease-in-out infinite;
  }
}
@keyframes mem-pulse {
  50% {
    opacity: 0.35;
  }
}
.memory-circle-value {
  position: relative;
  z-index: 1;
  font-size: 10px;
  font-weight: 700;
  letter-spacing: -0.02em;
  line-height: 1;
  font-variant-numeric: tabular-nums;
}
.controls {
  display: flex;
  height: 40px;
  width: var(--window-controls-width);
  align-items: stretch;
  flex: none;
  margin-top: 0;
}
.ctrl {
  display: grid;
  width: 46px;
  height: 100%;
  place-items: center;
  border: 0;
  background: transparent;
  color: color-mix(in oklab, var(--foreground) 72%, transparent);
  cursor: pointer;
}
.ctrl:hover {
  background: color-mix(in oklab, var(--muted) 75%, transparent);
  color: var(--foreground);
}
.ctrl.close:hover {
  background: var(--destructive);
  color: var(--destructive-foreground, white);
}
</style>

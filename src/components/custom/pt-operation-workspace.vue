<script setup lang="ts">
import { computed } from 'vue';
import type { Component } from 'vue';

const props = withDefaults(
  defineProps<{
    status: string;
    title: string;
    sourceLabel?: string;
    sourceValue?: string;
    sourceIcon?: Component;
    progress?: number;
    hint?: string;
    stats?: Array<{ label: string; value: string }>;
    icon?: Component;
    cancelLabel?: string;
    /** When false, hide the cancel control (read-only long loads). */
    cancellable?: boolean;
  }>(),
  {
    sourceLabel: undefined,
    sourceValue: undefined,
    sourceIcon: undefined,
    progress: 0,
    hint: undefined,
    stats: () => [],
    icon: undefined,
    cancelLabel: 'Cancel',
    cancellable: true,
  }
);

const emit = defineEmits<{ cancel: [] }>();

/** Circle radius in the 48×48 viewBox — circumference drives dashoffset. */
const RING_R = 20;
const RING_C = 2 * Math.PI * RING_R;

const progressClamped = computed(() =>
  Math.min(100, Math.max(0, Number(props.progress) || 0))
);

const progressOffset = computed(() => {
  const p = progressClamped.value / 100;
  return RING_C * (1 - Math.max(0.06, p));
});

const barWidth = computed(() => Math.min(100, Math.max(8, progressClamped.value)));
</script>

<template>
  <div class="op-stage">
    <div class="op-card">
      <div class="op-head">
        <div
          class="ring-wrap"
          role="progressbar"
          :aria-valuenow="Math.round(progressClamped)"
          aria-valuemin="0"
          aria-valuemax="100"
          :aria-label="status"
        >
          <div class="ring-disc" aria-hidden="true" />
          <svg class="ring" viewBox="0 0 48 48" aria-hidden="true">
            <circle class="spin-arc" cx="24" cy="24" :r="RING_R" />
          </svg>
          <svg class="ring ring-progress" viewBox="0 0 48 48" aria-hidden="true">
            <circle class="ring-track" cx="24" cy="24" :r="RING_R" />
            <circle
              class="ring-value"
              cx="24"
              cy="24"
              :r="RING_R"
              :style="{
                strokeDasharray: `${RING_C}`,
                strokeDashoffset: `${progressOffset}`,
              }"
            />
          </svg>
          <div class="ring-icon">
            <component :is="icon" v-if="icon" :size="18" :stroke-width="2" />
          </div>
        </div>
        <div class="op-copy">
          <div class="status">{{ status }}</div>
          <div class="title">{{ title }}</div>
        </div>
      </div>

      <div v-if="sourceValue" class="source">
        <span v-if="sourceIcon" class="source-glyph" aria-hidden="true">
          <component :is="sourceIcon" :size="15" :stroke-width="1.9" />
        </span>
        <div class="source-copy">
          <span class="source-label">{{ sourceLabel }}</span>
          <code>{{ sourceValue }}</code>
        </div>
      </div>

      <div class="bar" aria-hidden="true">
        <span :style="{ width: `${barWidth}%` }" />
      </div>

      <div v-if="stats?.length" class="stats">
        <div v-for="stat in stats" :key="stat.label" class="stat">
          <span>{{ stat.label }}</span>
          <strong>{{ stat.value }}</strong>
        </div>
      </div>

      <p v-if="hint" class="hint">{{ hint }}</p>

      <button
        v-if="cancellable"
        type="button"
        class="cancel"
        @click="emit('cancel')"
      >
        {{ cancelLabel }}
      </button>
    </div>
  </div>
</template>

<style scoped>
.op-stage {
  display: grid;
  place-items: center;
  width: 100%;
  height: 100%;
  min-height: 0;
  padding: 16px;
  animation: op-fade-in 220ms ease-out;
}
.op-card {
  width: min(520px, 100%);
  border: 1px solid color-mix(in oklab, var(--border) 70%, transparent);
  border-radius: 18px;
  background: var(--card);
  box-shadow: var(--shadow-card);
  padding: 26px 26px 20px;
}
.op-head {
  display: flex;
  align-items: center;
  gap: 14px;
}

/* Perfect circle — never rotate a square SVG (that paints spinning boxes). */
.ring-wrap {
  position: relative;
  width: 52px;
  height: 52px;
  flex: none;
  border-radius: 50%;
  overflow: hidden;
  isolation: isolate;
}
.ring-disc {
  position: absolute;
  inset: 4px;
  border-radius: 50%;
  background: color-mix(in oklab, var(--primary) 10%, var(--card));
  animation: ring-breathe 1.8s ease-in-out infinite;
}
.ring {
  position: absolute;
  inset: 0;
  width: 52px;
  height: 52px;
  display: block;
  border: 0;
  outline: none;
  overflow: hidden;
  pointer-events: none;
}
.ring-progress {
  transform: rotate(-90deg);
}
.ring-track,
.ring-value,
.spin-arc {
  fill: none;
  stroke-linecap: round;
}
.ring-track {
  stroke: color-mix(in oklab, var(--primary) 16%, var(--muted));
  stroke-width: 3;
}
.ring-value {
  stroke: var(--primary);
  stroke-width: 3.25;
  transition: stroke-dashoffset 420ms cubic-bezier(0.22, 1, 0.36, 1);
}
/* Indeterminate motion via dashoffset on the arc only — stays circular. */
.spin-arc {
  stroke: color-mix(in oklab, var(--primary) 55%, transparent);
  stroke-width: 2.5;
  stroke-dasharray: 31.4 94.2; /* ~1/4 of 2πr */
  animation: spin-dash 1s linear infinite;
}
.ring-icon {
  position: absolute;
  inset: 0;
  display: grid;
  place-items: center;
  border-radius: 50%;
  color: var(--primary);
  pointer-events: none;
  z-index: 1;
}

.op-copy .status {
  color: var(--primary);
  font-size: 0.75rem;
  font-weight: 700;
}
.op-copy .title {
  margin-top: 2px;
  font-size: 1.125rem;
  font-weight: 700;
  letter-spacing: -0.02em;
  color: var(--foreground);
}

.source {
  display: flex;
  align-items: center;
  gap: 10px;
  margin-top: 18px;
  border-radius: 12px;
  background: var(--surface-soft);
  padding: 12px 14px;
}
.source-glyph {
  display: grid;
  flex: none;
  width: 28px;
  height: 28px;
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
  overflow: hidden;
  color: var(--foreground);
  font-family: ui-monospace, 'Cascadia Code', Consolas, monospace;
  font-size: 0.75rem;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.bar {
  margin-top: 12px;
  height: 4px;
  border-radius: 999px;
  background: color-mix(in oklab, var(--primary) 16%, var(--muted));
  overflow: hidden;
}
.bar span {
  display: block;
  height: 100%;
  border-radius: inherit;
  background: linear-gradient(
    90deg,
    color-mix(in oklab, var(--primary) 78%, transparent),
    var(--primary)
  );
  transition: width 420ms cubic-bezier(0.22, 1, 0.36, 1);
}

.stats {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(0, 1fr));
  gap: 8px;
  margin-top: 14px;
}
.stat {
  border-radius: 12px;
  background: var(--surface-soft);
  padding: 10px 12px;
}
.stat span {
  display: block;
  color: var(--muted-foreground);
  font-size: 0.6875rem;
}
.stat strong {
  display: block;
  margin-top: 4px;
  font-size: 0.9375rem;
  font-weight: 700;
  color: var(--foreground);
  font-variant-numeric: tabular-nums;
}

.hint {
  margin: 16px 0 0;
  text-align: center;
  color: var(--muted-foreground);
  font-size: 0.75rem;
}
.cancel {
  width: 100%;
  margin-top: 16px;
  min-height: 44px;
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

@keyframes spin-dash {
  to {
    stroke-dashoffset: -125.6; /* full circumference */
  }
}
@keyframes ring-breathe {
  0%,
  100% {
    opacity: 0.85;
  }
  50% {
    opacity: 1;
  }
}
@keyframes op-fade-in {
  from {
    opacity: 0;
  }
  to {
    opacity: 1;
  }
}

@media (prefers-reduced-motion: reduce) {
  .op-stage,
  .spin-arc,
  .ring-disc,
  .ring-value,
  .bar span {
    animation: none;
    transition: none;
  }
}
</style>

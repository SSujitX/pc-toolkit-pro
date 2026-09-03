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
    /** Stable centered activity bar — no left→right growth or slide. */
    indeterminate?: boolean;
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
    indeterminate: false,
    hint: undefined,
    stats: () => [],
    icon: undefined,
    cancelLabel: 'Cancel',
    cancellable: true,
  }
);

const emit = defineEmits<{ cancel: [] }>();

const progressClamped = computed(() =>
  Math.min(100, Math.max(0, Number(props.progress) || 0))
);

/** Horizontal bar only — the header ring is an indeterminate “working” spinner. */
const barWidth = computed(() => Math.min(100, Math.max(0, progressClamped.value)));

/** Keep the path row visually stable while deep paths flash by. */
const displaySource = computed(() => {
  const value = (props.sourceValue || '').trim();
  if (!value) return '—';
  const max = 56;
  if (value.length <= max) return value;
  const keep = Math.floor((max - 1) / 2);
  return `${value.slice(0, keep)}…${value.slice(-keep)}`;
});
</script>

<template>
  <div class="op-stage">
    <div class="op-card">
      <div class="op-head">
        <div
          class="ring-wrap"
          role="progressbar"
          :aria-valuenow="indeterminate ? undefined : Math.round(progressClamped)"
          aria-valuemin="0"
          aria-valuemax="100"
          :aria-label="status"
        >
          <div class="ring-disc" aria-hidden="true" />
          <!-- Circular working spinner (not progress-tied). Progress lives on the bar below. -->
          <div class="ring-spinner" aria-hidden="true" />
          <div class="ring-icon">
            <component :is="icon" v-if="icon" :size="18" :stroke-width="2" />
          </div>
        </div>
        <div class="op-copy">
          <div class="status">{{ status }}</div>
          <div class="title">{{ title }}</div>
        </div>
      </div>

      <div v-if="sourceLabel || sourceValue" class="source">
        <span v-if="sourceIcon" class="source-glyph" aria-hidden="true">
          <component :is="sourceIcon" :size="15" :stroke-width="1.9" />
        </span>
        <div class="source-copy">
          <span class="source-label">{{ sourceLabel }}</span>
          <code :title="sourceValue || undefined">{{ displaySource }}</code>
        </div>
      </div>

      <div class="bar" :class="{ 'bar--indeterminate': indeterminate }" aria-hidden="true">
        <span v-if="!indeterminate" :style="{ width: `${barWidth}%` }" />
        <span v-else class="bar-center" />
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
  justify-items: center;
  width: 100%;
  height: 100%;
  min-height: 0;
  padding: 16px;
  animation: op-fade-in 220ms ease-out;
}
.op-card {
  box-sizing: border-box;
  width: min(520px, 100%);
  max-width: 100%;
  border: 1px solid color-mix(in oklab, var(--border) 70%, transparent);
  border-radius: 18px;
  background: var(--card);
  box-shadow: var(--shadow-card);
  padding: 26px 26px 20px;
  /* Prevent path/stat updates from shifting the centered card. */
  contain: layout style;
}
.op-head {
  display: flex;
  align-items: center;
  gap: 14px;
}

.ring-wrap {
  position: relative;
  width: 52px;
  height: 52px;
  flex: none;
  border-radius: 50%;
  isolation: isolate;
}
.ring-wrap::before {
  content: '';
  position: absolute;
  inset: 0;
  z-index: 0;
  border-radius: 50%;
  border: 3px solid color-mix(in oklab, var(--primary) 16%, var(--muted));
  pointer-events: none;
}
.ring-disc {
  position: absolute;
  inset: 4px;
  border-radius: 50%;
  background: color-mix(in oklab, var(--primary) 10%, var(--card));
  animation: ring-breathe 1.8s ease-in-out infinite;
}
/* Circular spinner: rotate the `rotate` property (not a square SVG / overflow clip). */
.ring-spinner {
  box-sizing: border-box;
  position: absolute;
  inset: 0;
  border-radius: 50%;
  background: conic-gradient(
    from 0deg,
    var(--primary) 0deg,
    color-mix(in oklab, var(--primary) 50%, transparent) 80deg,
    transparent 125deg
  );
  -webkit-mask: radial-gradient(farthest-side, transparent calc(100% - 3px), #000 calc(100% - 2.5px));
  mask: radial-gradient(farthest-side, transparent calc(100% - 3px), #000 calc(100% - 2.5px));
  transform-origin: 50% 50%;
  z-index: 1;
  animation: ring-spin 0.75s linear infinite;
  pointer-events: none;
}
.ring-icon {
  position: absolute;
  inset: 0;
  z-index: 2;
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
  min-height: 52px;
  border-radius: 12px;
  background: var(--surface-soft);
  padding: 12px 14px;
  overflow: hidden;
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
  overflow: hidden;
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
  line-height: 1.25;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.bar {
  position: relative;
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
/* Fixed centered segment — pulse only, never slides left/right. */
.bar--indeterminate {
  display: grid;
  place-items: center;
}
.bar--indeterminate .bar-center {
  width: 36%;
  transition: none;
  animation: bar-pulse 1.4s ease-in-out infinite;
}

.stats {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(0, 1fr));
  gap: 8px;
  margin-top: 14px;
}
.stat {
  min-width: 0;
  border-radius: 12px;
  background: var(--surface-soft);
  padding: 10px 12px;
  overflow: hidden;
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
  min-height: 1.25em;
  overflow: hidden;
  font-size: 0.9375rem;
  font-weight: 700;
  color: var(--foreground);
  font-variant-numeric: tabular-nums;
  text-overflow: ellipsis;
  white-space: nowrap;
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

@keyframes ring-spin {
  from {
    rotate: 0deg;
  }
  to {
    rotate: 360deg;
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
@keyframes bar-pulse {
  0%,
  100% {
    opacity: 0.55;
  }
  50% {
    opacity: 1;
  }
}

@media (prefers-reduced-motion: reduce) {
  .op-stage,
  .ring-disc,
  .bar span,
  .bar--indeterminate .bar-center {
    animation: none;
    transition: none;
  }
  /* Keep the busy ring moving — it is the working indicator, not decorative motion. */
  .ring-spinner {
    animation-duration: 1.25s;
  }
  .bar--indeterminate .bar-center {
    opacity: 0.85;
  }
}
</style>

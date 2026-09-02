<script setup lang="ts">
import type { Component } from 'vue';

withDefaults(
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
  }
);

const emit = defineEmits<{ cancel: [] }>();
</script>

<template>
  <div class="op-stage">
    <div class="op-card">
      <div class="op-head">
        <div class="ring-wrap">
          <svg class="ring" viewBox="0 0 48 48" aria-hidden="true">
            <circle class="ring-track" cx="24" cy="24" r="20" />
            <circle
              class="ring-value"
              cx="24"
              cy="24"
              r="20"
              :style="{
                strokeDasharray: `${Math.max(10, (progress / 100) * 126)} 126`,
              }"
            />
          </svg>
          <div class="ring-icon">
            <component :is="icon" v-if="icon" :size="20" :stroke-width="2" />
          </div>
        </div>
        <div class="op-copy">
          <div class="status">{{ status }}</div>
          <div class="title">{{ title }}</div>
        </div>
      </div>

      <div v-if="sourceValue" class="source">
        <component
          :is="sourceIcon"
          v-if="sourceIcon"
          class="source-icon"
          :size="16"
          :stroke-width="1.8"
        />
        <div class="source-copy">
          <span class="source-label">{{ sourceLabel }}</span>
          <code>{{ sourceValue }}</code>
        </div>
        <span class="source-elapsed" />
      </div>

      <div class="bar">
        <span :style="{ width: `${Math.min(100, Math.max(6, progress))}%` }" />
      </div>

      <div v-if="stats?.length" class="stats">
        <div v-for="stat in stats" :key="stat.label" class="stat">
          <span>{{ stat.label }}</span>
          <strong>{{ stat.value }}</strong>
        </div>
      </div>

      <p v-if="hint" class="hint">{{ hint }}</p>

      <button type="button" class="cancel" @click="emit('cancel')">
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
.ring-wrap {
  position: relative;
  width: 52px;
  height: 52px;
  flex: none;
}
.ring {
  width: 52px;
  height: 52px;
  transform: rotate(-90deg);
}
.ring-track,
.ring-value {
  fill: none;
  stroke-width: 3.25;
}
.ring-track {
  stroke: color-mix(in oklab, var(--primary) 18%, var(--muted));
}
.ring-value {
  stroke: var(--primary);
  stroke-linecap: round;
}
.ring-icon {
  position: absolute;
  inset: 0;
  display: grid;
  place-items: center;
  color: var(--primary);
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
.source-icon {
  flex: none;
  color: var(--muted-foreground);
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
  background: var(--primary);
}
.stats {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
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
</style>

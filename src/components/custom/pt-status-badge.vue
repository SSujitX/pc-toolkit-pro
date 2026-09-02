<script setup lang="ts">
defineProps<{ label: string; tone?: 'default' | 'success' | 'warning' | 'danger' }>();
</script>
<template>
  <span class="badge" :data-tone="tone ?? 'default'">{{ label }}</span>
</template>
<style scoped>
.badge {
  display: inline-flex;
  align-items: center;
  height: 22px;
  padding: 0 8px;
  border-radius: 999px;
  font-size: 0.6875rem;
  font-weight: 600;
  background: var(--muted);
  color: var(--muted-foreground);
}
.badge[data-tone='success'] {
  background: color-mix(in oklab, var(--success) 18%, transparent);
  color: var(--success);
}
.badge[data-tone='warning'] {
  background: color-mix(in oklab, var(--warning) 18%, transparent);
  color: var(--warning);
}
.badge[data-tone='danger'] {
  background: color-mix(in oklab, var(--destructive) 18%, transparent);
  color: var(--destructive);
}
</style>

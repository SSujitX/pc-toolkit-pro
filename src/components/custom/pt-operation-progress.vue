<script setup lang="ts">
defineProps<{ current: number; total: number; message?: string }>();
</script>
<template>
  <div class="pt-progress" role="status">
    <div class="track"><span :style="{ width: `${total ? (current / total) * 100 : 0}%` }" /></div>
    <p>{{ message ?? `${current} / ${total}` }}</p>
  </div>
</template>
<style scoped>
.pt-progress {
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.track {
  height: 6px;
  border-radius: 999px;
  background: var(--muted);
  overflow: hidden;
}
.track span {
  display: block;
  height: 100%;
  background: var(--primary);
}
p {
  margin: 0;
  font-size: 0.75rem;
  color: var(--muted-foreground);
}
</style>

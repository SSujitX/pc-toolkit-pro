<script setup lang="ts">
defineProps<{ title: string; description?: string }>();
</script>
<template>
  <div class="pt-empty">
    <h3>{{ title }}</h3>
    <p v-if="description">{{ description }}</p>
    <slot />
  </div>
</template>
<style scoped>
.pt-empty {
  display: grid;
  place-items: center;
  gap: 6px;
  min-height: 160px;
  color: var(--muted-foreground);
  text-align: center;
  font-size: 0.8125rem;
}
h3 {
  margin: 0;
  color: var(--foreground);
  font-size: 1rem;
}
p {
  margin: 0;
}
</style>

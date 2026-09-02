<script setup lang="ts">
defineProps<{ label: string }>();
</script>

<template>
  <span class="pt-icon" :title="label" aria-hidden="true">
    <slot />
  </span>
</template>

<style scoped>
.pt-icon {
  display: inline-grid;
  width: 22px;
  height: 22px;
  place-items: center;
  font-size: 14px;
  line-height: 1;
}
</style>

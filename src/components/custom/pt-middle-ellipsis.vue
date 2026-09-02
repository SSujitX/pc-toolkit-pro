<script setup lang="ts">
defineProps<{ text: string }>();
</script>
<template>
  <span class="mid" :title="text">{{ text }}</span>
</template>
<style scoped>
.mid {
  display: inline-block;
  max-width: 100%;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  direction: rtl;
  text-align: left;
  font-size: 0.75rem;
}
</style>

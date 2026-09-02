<script setup lang="ts"></script>
<template>
  <div class="workspace">
    <div v-if="$slots.toolbar" class="toolbar"><slot name="toolbar" /></div>
    <div class="body"><slot /></div>
    <div v-if="$slots.footer" class="footer"><slot name="footer" /></div>
  </div>
</template>
<style scoped>
.workspace {
  display: flex;
  min-height: 0;
  flex: 1;
  flex-direction: column;
}
.toolbar {
  display: flex;
  height: var(--layout-workspace-toolbar-height);
  flex: none;
  align-items: center;
  gap: 8px;
}
.body {
  min-height: 0;
  flex: 1;
  overflow: auto;
}
.footer {
  display: flex;
  min-height: var(--layout-action-bar-height);
  flex: none;
  align-items: center;
  justify-content: flex-end;
  gap: 8px;
}
</style>

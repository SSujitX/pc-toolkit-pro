<script setup lang="ts">
withDefaults(
  defineProps<{
    title: string;
    subtitle?: string;
    contentMode?: 'document' | 'workspace';
  }>(),
  { subtitle: undefined, contentMode: 'document' }
);
</script>

<template>
  <section class="pt-page-shell" :data-mode="contentMode">
    <header class="pt-page-header" data-tauri-drag-region>
      <div class="pt-page-heading">
        <h1>{{ title }}</h1>
        <p v-if="subtitle">{{ subtitle }}</p>
      </div>
      <div class="pt-page-actions">
        <slot name="actions" />
      </div>
    </header>
    <div class="pt-page-content" :class="`pt-page-content--${contentMode}`">
      <slot />
    </div>
    <footer v-if="$slots.footer" class="pt-page-footer">
      <slot name="footer" />
    </footer>
  </section>
</template>

<style scoped>
@reference "@assets/main.css";
.pt-page-shell {
  display: flex;
  width: 100%;
  height: 100%;
  min-height: 0;
  flex-direction: column;
  padding: 0 var(--layout-page-padding-inline);
}
.pt-page-header {
  display: grid;
  min-height: var(--layout-page-header-height);
  grid-template-columns: minmax(0, 1fr) auto;
  align-items: center;
  gap: 14px;
  flex: none;
  padding-inline-end: calc(var(--window-controls-width) + 12px);
}
.pt-page-heading h1 {
  margin: 0;
  font-size: 22px;
  font-weight: 650;
  letter-spacing: -0.02em;
}
.pt-page-heading p {
  margin: 2px 0 0;
  color: var(--muted-foreground);
  font-size: 0.75rem;
}
.pt-page-actions {
  display: flex;
  gap: 8px;
  align-items: center;
}
.pt-page-content {
  display: flex;
  min-height: 0;
  flex: 1;
  flex-direction: column;
  gap: 12px;
  overflow: auto;
  padding-bottom: 16px;
}
.pt-page-content--workspace {
  overflow: hidden;
}
.pt-page-footer {
  display: flex;
  flex: none;
  justify-content: flex-end;
  gap: 10px;
  min-height: var(--layout-action-bar-height);
  align-items: center;
  padding-bottom: 10px;
}
</style>

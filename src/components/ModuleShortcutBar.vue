<script setup lang="ts">
export interface ShortcutItem {
  keys: string[];
  label: string;
}

defineProps<{
  items: ShortcutItem[];
}>();
</script>

<template>
  <footer class="shortcutbar">
    <div class="hints">
      <span v-for="item in items" :key="`${item.label}-${item.keys.join('-')}`">
        <kbd v-for="key in item.keys" :key="`${item.label}-${key}`">{{ key }}</kbd>
        {{ item.label }}
      </span>
    </div>
  </footer>
</template>

<style scoped>
.shortcutbar {
  display: flex;
  justify-content: flex-end;
  padding: 8px 4px 4px;
  color: var(--text-dim);
  font-size: 12px;
}

.hints {
  display: flex;
  flex-wrap: wrap;
  justify-content: flex-end;
  gap: 13px;
  font-size: 11px;
  color: var(--text-dim);
}

.hints span {
  display: inline-flex;
  align-items: center;
  gap: 4px;
}

kbd {
  display: inline-block;
  min-width: 18px;
  padding: 1px 5px;
  text-align: center;
  background: var(--panel);
  border: 1px solid var(--border);
  border-bottom-width: 2px;
  border-radius: 5px;
  font: inherit;
  font-size: 10.5px;
  line-height: 1.35;
  color: var(--text);
}

@media (max-width: 920px) {
  .shortcutbar {
    justify-content: flex-start;
  }

  .hints {
    justify-content: flex-start;
  }
}
</style>

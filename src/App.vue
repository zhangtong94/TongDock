<script setup lang="ts">
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import ModuleShortcutBar, { type ShortcutItem } from "./components/ModuleShortcutBar.vue";
import ClipTrailModule from "./modules/ClipTrailModule.vue";
import MarkLensModule from "./modules/MarkLensModule.vue";
import TextForgeModule from "./modules/TextForgeModule.vue";
import appIcon from "./assets/tongdock-logo.svg";

type ModuleId = "cliptrail" | "marklens" | "textforge";
type ThemeMode = "dark" | "light";

interface BenchMetric {
  value: number | string;
  label: string;
}

interface BenchModule {
  id: ModuleId;
  name: string;
  note: string;
  component: object;
  shortcuts: ShortcutItem[];
}

const moduleStorageKey = "tongdock-active-module";
const themeStorageKey = "tongdock-theme";
const modules: BenchModule[] = [
  {
    id: "cliptrail",
    name: "ClipTrail",
    note: "clipboard capture, search, and recall",
    component: ClipTrailModule,
    shortcuts: [
      { keys: ["↑", "↓"], label: "move" },
      { keys: ["↵"], label: "copy" },
      { keys: ["⌘P"], label: "pin" },
      { keys: ["⌘⌫"], label: "del" },
      { keys: ["esc"], label: "hide" },
    ],
  },
  {
    id: "marklens",
    name: "MarkLens",
    note: "markdown reading and live preview",
    component: MarkLensModule,
    shortcuts: [
      { keys: ["⌘/Ctrl", "O"], label: "open" },
      { keys: ["⌘/Ctrl", "S"], label: "save" },
      { keys: ["⌘/Ctrl", "K"], label: "clear" },
    ],
  },
  {
    id: "textforge",
    name: "TextForge",
    note: "text conversion, formatting, and SQL helpers",
    component: TextForgeModule,
    shortcuts: [
      { keys: ["⌘/Ctrl", "↵"], label: "copy" },
      { keys: ["⌘/Ctrl", "⇧", "↵"], label: "reuse" },
      { keys: ["⌘/Ctrl", "K"], label: "clear" },
    ],
  },
];

const currentModuleId = ref<ModuleId>("cliptrail");
const theme = ref<ThemeMode>("dark");
const headerMetrics = ref<BenchMetric[]>([]);
let unlistenNavigate: UnlistenFn | null = null;

function isModuleId(value: string): value is ModuleId {
  return modules.some((module) => module.id === value);
}

function setModule(id: ModuleId) {
  currentModuleId.value = id;
  headerMetrics.value = [];
  localStorage.setItem(moduleStorageKey, id);
}

function applyTheme(mode: ThemeMode) {
  document.documentElement.dataset.theme = mode;
}

function toggleTheme() {
  theme.value = theme.value === "dark" ? "light" : "dark";
}

function updateHeaderMetrics(metrics: BenchMetric[]) {
  headerMetrics.value = metrics;
}

const currentModule = computed(
  () => modules.find((module) => module.id === currentModuleId.value) ?? modules[0],
);

watch(theme, (value) => {
  applyTheme(value);
  localStorage.setItem(themeStorageKey, value);
});

onMounted(async () => {
  const stored = localStorage.getItem(moduleStorageKey);
  const storedTheme = localStorage.getItem(themeStorageKey);

  if (stored && isModuleId(stored)) {
    currentModuleId.value = stored;
  }

  theme.value =
    storedTheme === "light" || storedTheme === "dark"
      ? storedTheme
      : window.matchMedia("(prefers-color-scheme: light)").matches
        ? "light"
        : "dark";
  applyTheme(theme.value);

  unlistenNavigate = await listen<string>("tongdock:navigate", (event) => {
    if (isModuleId(event.payload)) {
      setModule(event.payload);
    }
  });
});

onBeforeUnmount(() => {
  unlistenNavigate?.();
});
</script>

<template>
  <div class="bench">
    <header class="benchbar">
      <div class="benchbrand">
        <span class="benchlogo" aria-hidden="true">
          <img :src="appIcon" alt="" />
        </span>
        <div class="benchcopy">
          <div class="benchname-row">
            <span class="benchname">Tong<span class="accent">Dock</span></span>
          </div>
          <p class="benchnote">{{ currentModule.note }}</p>
        </div>
      </div>

      <nav class="modulenav" aria-label="tongdock modules">
        <button
          v-for="module in modules"
          :key="module.id"
          class="modulepill"
          :class="{ active: module.id === currentModule.id }"
          @click="setModule(module.id)"
        >
          {{ module.name }}
        </button>
      </nav>

      <div class="benchright">
        <div class="modulemetrics">
          <div v-for="metric in headerMetrics" :key="`${metric.label}-${metric.value}`" class="stat">
            <span class="count">{{ metric.value }}</span> {{ metric.label }}
          </div>
        </div>
        <button
          class="theme-btn"
          :title="theme === 'dark' ? 'switch to light' : 'switch to dark'"
          @click="toggleTheme"
        >
          {{ theme === "dark" ? "☀" : "☾" }}
        </button>
      </div>
    </header>

    <main class="modulehost">
      <div class="modulesurface">
        <component
          :is="currentModule.component"
          :key="currentModule.id"
          :embedded="true"
          :shared-theme="theme"
          @header-metrics="updateHeaderMetrics"
        />
      </div>
    </main>

    <ModuleShortcutBar :items="currentModule.shortcuts" />
  </div>
</template>

<style scoped>
.bench {
  height: 100%;
  display: flex;
  flex-direction: column;
  background: var(--bg);
}

.benchbar {
  display: grid;
  grid-template-columns: minmax(0, 1fr) auto minmax(0, 1fr);
  align-items: center;
  gap: 14px;
  padding: 8px 14px;
  border-bottom: 1px solid var(--border-soft);
  background: var(--bg-elev);
}

.benchbrand,
.benchname-row,
.modulenav,
.benchright,
.modulemetrics {
  display: flex;
  align-items: center;
}

.benchbrand {
  min-width: 0;
  gap: 10px;
  justify-self: start;
}

.benchlogo {
  width: 32px;
  height: 32px;
  display: grid;
  place-items: center;
  border: 1px solid var(--border);
  border-radius: 10px;
  background: var(--panel);
  color: var(--accent);
  box-shadow: inset 0 0 0 1px var(--accent-glow);
}

.benchlogo img {
  width: 24px;
  height: 24px;
  display: block;
}

.benchcopy {
  min-width: 0;
}

.benchname-row {
  gap: 8px;
}

.benchname {
  font-size: 16px;
  font-weight: 700;
  letter-spacing: -0.03em;
}

.accent {
  color: var(--accent);
}

.benchnote {
  margin: 1px 0 0;
  color: var(--text-dim);
  font-size: 11px;
}

.modulenav {
  justify-self: center;
  justify-content: center;
  flex-wrap: wrap;
  gap: 8px;
}

.benchright {
  justify-self: end;
  justify-content: flex-end;
  gap: 10px;
}

.modulemetrics {
  flex-wrap: wrap;
  justify-content: flex-end;
  gap: 8px;
}

.stat,
.theme-btn {
  border: 1px solid var(--border);
  background: var(--panel);
}

.stat {
  padding: 6px 9px;
  border-radius: 9px;
  color: var(--text-dim);
  white-space: nowrap;
}

.count {
  color: var(--text);
  font-weight: 600;
}

.theme-btn {
  width: 30px;
  height: 30px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border-radius: 9px;
  color: var(--text-dim);
  font: inherit;
  transition:
    border-color 120ms ease,
    color 120ms ease,
    box-shadow 120ms ease,
    background-color 120ms ease;
}

.modulepill {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  padding: 6px 11px;
  border: 1px solid var(--border);
  border-radius: 999px;
  background: var(--panel);
  color: var(--text-dim);
  font: inherit;
  transition:
    border-color 120ms ease,
    color 120ms ease,
    box-shadow 120ms ease,
    background-color 120ms ease;
}

.modulepill:hover,
.modulepill:focus-visible {
  border-color: var(--accent-dim);
  color: var(--text);
  box-shadow: 0 0 0 4px var(--accent-glow);
  outline: none;
}

.modulepill.active {
  border-color: var(--accent-dim);
  color: var(--accent);
  box-shadow: 0 0 0 4px var(--accent-glow);
}

.theme-btn:hover,
.theme-btn:focus-visible {
  border-color: var(--accent-dim);
  color: var(--text);
  box-shadow: 0 0 0 4px var(--accent-glow);
  outline: none;
}

.modulehost {
  flex: 1;
  min-height: 0;
}

.modulesurface {
  height: 100%;
  min-height: 0;
  overflow: hidden;
}

.modulesurface :deep(.app) {
  height: 100%;
}

@media (max-width: 1100px) {
  .benchbar {
    grid-template-columns: minmax(0, 1fr);
    align-items: stretch;
  }

  .modulenav {
    justify-content: flex-start;
  }

  .benchright,
  .modulemetrics {
    justify-content: flex-start;
  }
}
</style>

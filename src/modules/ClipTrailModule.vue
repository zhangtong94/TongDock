<script setup lang="ts">
import { ref, computed, watch, onMounted, onBeforeUnmount, nextTick } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

type ThemeMode = "dark" | "light";

interface HeaderMetric {
  value: number | string;
  label: string;
}

interface ClipItem {
  id: number;
  kind: "text" | "image" | "files";
  content: string;
  image_data_url: string | null;
  image_width: number | null;
  image_height: number | null;
  image_format: string | null;
  byte_size: number | null;
  file_paths: string[];
  pinned: boolean;
  ts: number;
}

type FilterScope = "all" | ClipItem["kind"];
type SectionScope = Exclude<FilterScope, "all">;
const SECTION_ORDER_STORAGE_KEY = "tongdock-cliptrail-section-order-v1";

const props = withDefaults(defineProps<{ embedded?: boolean; sharedTheme?: ThemeMode }>(), {
  embedded: false,
  sharedTheme: "dark",
});

const emit = defineEmits<{
  (e: "header-metrics", metrics: HeaderMetric[]): void;
}>();

const items = ref<ClipItem[]>([]);
const query = ref("");
const scope = ref<FilterScope>("all");
const selected = ref(0);
const toast = ref("");
const now = ref(Date.now());
const theme = ref<ThemeMode>("dark");

const searchEl = ref<HTMLInputElement | null>(null);
const listEl = ref<HTMLElement | null>(null);

let unlisten: UnlistenFn | null = null;
let toastTimer: number | undefined;
let clockTimer: number | undefined;

const filtered = computed(() => {
  const q = query.value.trim().toLowerCase();
  const list = items.value.filter((item) => {
    const matchesQuery = !q || searchableText(item).includes(q);
    const matchesScope = scope.value === "all" || item.kind === scope.value;
    return matchesQuery && matchesScope;
  });
  list.sort((a, b) => Number(b.pinned) - Number(a.pinned) || b.ts - a.ts);
  return list;
});

const filteredTextItems = computed(() => filtered.value.filter((item) => item.kind === "text"));
const filteredImageItems = computed(() => filtered.value.filter((item) => item.kind === "image"));
const filteredFileItems = computed(() => filtered.value.filter((item) => item.kind === "files"));
const sectionOrder = ref<SectionScope[]>(["text", "image", "files"]);
const draggingSection = ref<SectionScope | null>(null);
const dragOverSection = ref<SectionScope | null>(null);

const filteredIndexById = computed(() => {
  const map = new Map<number, number>();
  filtered.value.forEach((item, idx) => map.set(item.id, idx));
  return map;
});

const pinnedCount = computed(() => items.value.filter((i) => i.pinned).length);
const headerMetrics = computed<HeaderMetric[]>(() => [{ value: items.value.length, label: "entries" }]);

watch(query, () => {
  selected.value = 0;
});

watch(scope, () => {
  selected.value = 0;
});

function applyTheme(t: "dark" | "light") {
  document.documentElement.dataset.theme = t;
}

watch(theme, (t) => {
  applyTheme(t);
  if (!props.embedded) {
    localStorage.setItem("cliptrail-theme", t);
  }
});

watch(() => props.sharedTheme, (value) => {
  if (props.embedded && value && theme.value !== value) {
    theme.value = value;
  }
}, { immediate: true });

watch(sectionOrder, () => {
  persistSectionOrder();
}, { deep: true });

watch(headerMetrics, (metrics) => {
  emit("header-metrics", metrics);
}, { immediate: true });

function lineCount(s: string): number {
  return s.split("\n").length;
}

function searchableText(item: ClipItem): string {
  return [item.content, ...item.file_paths].join("\n").toLowerCase();
}

function setScope(next: FilterScope) {
  scope.value = next;
}

function scopeColumnTitle(kind: SectionScope): string {
  if (kind === "image") return "images";
  if (kind === "files") return "files";
  return "text";
}

function sectionItems(kind: SectionScope): ClipItem[] {
  if (kind === "text") return filteredTextItems.value;
  if (kind === "image") return filteredImageItems.value;
  return filteredFileItems.value;
}

function isSectionScope(value: string): value is SectionScope {
  return value === "text" || value === "image" || value === "files";
}

function persistSectionOrder() {
  localStorage.setItem(SECTION_ORDER_STORAGE_KEY, JSON.stringify(sectionOrder.value));
}

function loadSectionOrder() {
  const raw = localStorage.getItem(SECTION_ORDER_STORAGE_KEY);
  if (!raw) return;

  try {
    const parsed = JSON.parse(raw);
    if (!Array.isArray(parsed)) return;

    const allowed: SectionScope[] = ["text", "image", "files"];
    if (parsed.length !== allowed.length) return;
    const valid = parsed.every((item) => allowed.includes(item));
    const unique = new Set(parsed);

    if (valid && unique.size === allowed.length) {
      sectionOrder.value = parsed as SectionScope[];
    }
  } catch {
    /* ignore invalid persisted order */
  }
}

function reorderSection(source: SectionScope, target: SectionScope) {
  if (source === target) return;

  const next = [...sectionOrder.value];
  const from = next.indexOf(source);
  const to = next.indexOf(target);
  if (from < 0 || to < 0) return;

  next.splice(from, 1);
  next.splice(to, 0, source);
  sectionOrder.value = next;
}

function onSectionPointerMove(event: PointerEvent) {
  if (!draggingSection.value) return;

  const el = document.elementFromPoint(event.clientX, event.clientY) as HTMLElement | null;
  const column = el?.closest<HTMLElement>("[data-section-kind]");
  const kind = column?.dataset.sectionKind;
  if (!kind || !isSectionScope(kind)) return;

  dragOverSection.value = kind;
}

function onSectionPointerUp() {
  const source = draggingSection.value;
  const target = dragOverSection.value;

  if (source && target) {
    reorderSection(source, target);
  }

  draggingSection.value = null;
  dragOverSection.value = null;
  window.removeEventListener("pointermove", onSectionPointerMove);
  window.removeEventListener("pointerup", onSectionPointerUp);
}

function onSectionPointerDown(kind: SectionScope, event: PointerEvent) {
  if (event.button !== 0) return;
  event.preventDefault();

  draggingSection.value = kind;
  dragOverSection.value = kind;
  window.addEventListener("pointermove", onSectionPointerMove);
  window.addEventListener("pointerup", onSectionPointerUp);
}

function isScopeActive(next: FilterScope): boolean {
  return scope.value === next;
}

function scopeTitle(next: FilterScope): string {
  if (next === "all") return "show all history";
  if (next === "text") return "show text history";
  if (next === "image") return "show image history";
  return "show file history";
}

function emptyMessage(): string {
  if (query.value) {
    return `no matches for “${query.value}”`;
  }

  if (scope.value === "text") return "no text clips yet";
  if (scope.value === "image") return "no image clips yet";
  if (scope.value === "files") return "no file clips yet";
  return "clipboard trail is empty — copy something to begin";
}

function kindLabel(item: ClipItem): string {
  if (item.kind === "image") return "image";
  if (item.kind === "files") return "files";
  return "text";
}

function copyMessage(item: ClipItem): string {
  if (item.kind === "image") return "image copied → clipboard";
  if (item.kind === "files") return "files copied → clipboard";
  return "copied → clipboard";
}

function formatBytes(bytes: number | null): string | null {
  if (!bytes || bytes <= 0) return null;
  if (bytes < 1024) return `${bytes} B`;

  const units = ["KB", "MB", "GB", "TB"];
  let value = bytes / 1024;
  let unit = units[0];

  for (let index = 1; index < units.length && value >= 1024; index += 1) {
    value /= 1024;
    unit = units[index];
  }

  const fixed = value >= 100 ? 0 : value >= 10 ? 1 : 2;
  return `${value.toFixed(fixed)} ${unit}`;
}

function imageCaption(item: ClipItem): string {
  const dimensions = item.image_width && item.image_height
    ? `${item.image_width}x${item.image_height}`
    : null;
  const size = formatBytes(item.byte_size);
  const format = item.image_format ? item.image_format.toUpperCase() : null;

  const parts = [dimensions, size, format].filter((value): value is string => Boolean(value));
  if (parts.length) {
    return parts.join(" · ");
  }

  return item.content;
}

function primaryMeta(item: ClipItem): string | null {
  if (item.kind === "image") {
    return null;
  }

  if (item.kind === "files") {
    const count = item.file_paths.length;
    if (!count) return null;
    return `${count} file${count === 1 ? "" : "s"}`;
  }

  return `${item.content.length} ch`;
}

function secondaryMeta(item: ClipItem): string | null {
  if (item.kind === "files") {
    return formatBytes(item.byte_size);
  }

  if (item.kind === "text" && lineCount(item.content) > 1) {
    return `${lineCount(item.content)} lines`;
  }
  return null;
}

function displayFileName(path: string): string {
  const parts = path.split(/[\\/]/);
  return parts[parts.length - 1] || path;
}

function previewFilePaths(item: ClipItem): string[] {
  return item.file_paths.slice(0, 3);
}

function selectedIndexFor(item: ClipItem): number {
  return filteredIndexById.value.get(item.id) ?? -1;
}

function isSelectedItem(item: ClipItem): boolean {
  return selectedIndexFor(item) === selected.value;
}

function selectItem(item: ClipItem) {
  const idx = selectedIndexFor(item);
  if (idx >= 0) {
    selected.value = idx;
  }
}

function relativeTime(ts: number, ref: number): string {
  const s = Math.floor((ref - ts) / 1000);
  if (s < 5) return "just now";
  if (s < 60) return `${s}s ago`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}m ago`;
  const h = Math.floor(m / 60);
  if (h < 24) return `${h}h ago`;
  const d = Math.floor(h / 24);
  if (d < 7) return `${d}d ago`;
  return new Date(ts).toLocaleDateString();
}

function flash(msg: string) {
  toast.value = msg;
  clearTimeout(toastTimer);
  toastTimer = window.setTimeout(() => (toast.value = ""), 1200);
}

function clampSelection() {
  const max = filtered.value.length - 1;
  selected.value = Math.min(Math.max(0, selected.value), Math.max(0, max));
}

function move(delta: number) {
  const len = filtered.value.length;
  if (!len) return;
  selected.value = (selected.value + delta + len) % len;
  nextTick(() => {
    listEl.value
      ?.querySelector<HTMLElement>(`[data-idx="${selected.value}"]`)
      ?.scrollIntoView({ block: "nearest" });
  });
}

async function refresh() {
  items.value = await invoke<ClipItem[]>("get_history");
  clampSelection();
}

async function copy(item: ClipItem) {
  await invoke("copy_item", { id: item.id });
  flash(copyMessage(item));
}

async function remove(item: ClipItem) {
  items.value = await invoke<ClipItem[]>("delete_item", { id: item.id });
  clampSelection();
}

async function pin(item: ClipItem) {
  items.value = await invoke<ClipItem[]>("toggle_pin", { id: item.id });
}

async function clearAll() {
  items.value = await invoke<ClipItem[]>("clear_history");
  clampSelection();
  flash("trail cleared");
}

function hide() {
  invoke("hide_window");
}

function focusSearch() {
  nextTick(() => searchEl.value?.focus());
}

function onKeydown(e: KeyboardEvent) {
  const item = filtered.value[selected.value];
  const mod = e.metaKey || e.ctrlKey;

  if (e.key === "ArrowDown") {
    e.preventDefault();
    move(1);
  } else if (e.key === "ArrowUp") {
    e.preventDefault();
    move(-1);
  } else if (e.key === "Enter") {
    if (item) {
      e.preventDefault();
      copy(item);
    }
  } else if (e.key === "Escape") {
    if (query.value) query.value = "";
    else hide();
  } else if (mod && (e.key === "Backspace" || e.key === "Delete")) {
    if (item) {
      e.preventDefault();
      remove(item);
    }
  } else if (mod && (e.key === "p" || e.key === "P")) {
    if (item) {
      e.preventDefault();
      pin(item);
    }
  }
}

onMounted(async () => {
  loadSectionOrder();

  if (props.embedded) {
    theme.value = props.sharedTheme;
  } else {
    const saved = localStorage.getItem("cliptrail-theme");
    theme.value =
      saved === "light" || saved === "dark"
        ? saved
        : window.matchMedia("(prefers-color-scheme: light)").matches
          ? "light"
          : "dark";
  }
  applyTheme(theme.value);

  await refresh();
  unlisten = await listen<ClipItem[]>("clipboard-updated", (e) => {
    items.value = e.payload;
    clampSelection();
  });
  window.addEventListener("keydown", onKeydown);
  window.addEventListener("focus", focusSearch);
  clockTimer = window.setInterval(() => (now.value = Date.now()), 20000);
  focusSearch();
});

onBeforeUnmount(() => {
  persistSectionOrder();
  window.removeEventListener("pointermove", onSectionPointerMove);
  window.removeEventListener("pointerup", onSectionPointerUp);
  unlisten?.();
  window.removeEventListener("keydown", onKeydown);
  window.removeEventListener("focus", focusSearch);
  clearInterval(clockTimer);
  clearTimeout(toastTimer);
});
</script>

<template>
  <div class="app">
    <div class="toolbar">
      <div class="searchbar">
        <span class="prompt">›</span>
        <input
          ref="searchEl"
          v-model="query"
          class="search"
          type="text"
          spellcheck="false"
          autocomplete="off"
          autocorrect="off"
          placeholder="search clipboard history…"
        />
      </div>

      <div class="toolbar-actions" role="tablist" aria-label="history type filter">
        <button
          class="action"
          :class="{ active: isScopeActive('all') }"
          :title="scopeTitle('all')"
          :aria-pressed="isScopeActive('all')"
          @click="setScope('all')"
        >
          <svg viewBox="0 0 20 20" aria-hidden="true">
            <path
              d="M4 5.5h12M4 10h12M4 14.5h12"
              fill="none"
              stroke="currentColor"
              stroke-linecap="round"
              stroke-width="1.7"
            />
          </svg>
        </button>
        <button
          class="action"
          :class="{ active: isScopeActive('text') }"
          :title="scopeTitle('text')"
          :aria-pressed="isScopeActive('text')"
          @click="setScope('text')"
        >
          <svg viewBox="0 0 20 20" aria-hidden="true">
            <path
              d="M4.5 5.5h11M10 5.5v9M6.5 14.5h7"
              fill="none"
              stroke="currentColor"
              stroke-linecap="round"
              stroke-linejoin="round"
              stroke-width="1.7"
            />
          </svg>
        </button>
        <button
          class="action"
          :class="{ active: isScopeActive('image') }"
          :title="scopeTitle('image')"
          :aria-pressed="isScopeActive('image')"
          @click="setScope('image')"
        >
          <svg viewBox="0 0 20 20" aria-hidden="true">
            <path
              d="M4.5 5.5h11a1 1 0 0 1 1 1v7a1 1 0 0 1-1 1h-11a1 1 0 0 1-1-1v-7a1 1 0 0 1 1-1Z"
              fill="none"
              stroke="currentColor"
              stroke-width="1.5"
            />
            <circle cx="7.3" cy="8.2" r="1.2" fill="currentColor" />
            <path
              d="m6 13 2.4-2.6 2.2 2 2.2-3 2.2 3.6"
              fill="none"
              stroke="currentColor"
              stroke-linecap="round"
              stroke-linejoin="round"
              stroke-width="1.5"
            />
          </svg>
        </button>
        <button
          class="action"
          :class="{ active: isScopeActive('files') }"
          :title="scopeTitle('files')"
          :aria-pressed="isScopeActive('files')"
          @click="setScope('files')"
        >
          <svg viewBox="0 0 20 20" aria-hidden="true">
            <path
              d="M6 4.5h5l3 3v8a1 1 0 0 1-1 1H6a1 1 0 0 1-1-1v-10a1 1 0 0 1 1-1Z"
              fill="none"
              stroke="currentColor"
              stroke-linejoin="round"
              stroke-width="1.5"
            />
            <path
              d="M11 4.5v3h3"
              fill="none"
              stroke="currentColor"
              stroke-linejoin="round"
              stroke-width="1.5"
            />
          </svg>
        </button>
        <button v-if="query" class="action secondary-action" title="clear search" @click="query = ''">
          <svg viewBox="0 0 20 20" aria-hidden="true">
            <path d="M6 6l8 8M14 6l-8 8" fill="none" stroke="currentColor" stroke-linecap="round" stroke-width="1.7" />
          </svg>
        </button>
        <button
          class="action danger-action"
          :disabled="items.length === pinnedCount"
          title="clear all unpinned entries"
          @click="clearAll"
        >
          <svg viewBox="0 0 20 20" aria-hidden="true">
            <path d="M6 6l8 8M14 6l-8 8" fill="none" stroke="currentColor" stroke-linecap="round" stroke-width="1.7" />
          </svg>
        </button>
      </div>
    </div>

    <main
      ref="listEl"
      class="list"
      :class="{ 'split-layout': scope === 'all', 'grid-layout': scope === 'image' || scope === 'files' }"
    >
      <template v-if="filtered.length">
        <template v-if="scope === 'all'">
          <section
            v-for="kind in sectionOrder"
            :key="kind"
            class="scope-column"
            :data-section-kind="kind"
            :class="{ 'is-drop-target': dragOverSection === kind }"
          >
            <header
              class="scope-column-head"
              @pointerdown="onSectionPointerDown(kind, $event)"
            >
              <span>{{ scopeColumnTitle(kind) }}</span>
              <span>{{ sectionItems(kind).length }}</span>
            </header>
            <div class="scope-column-list">
              <article
                v-for="item in sectionItems(kind)"
                :key="item.id"
                :data-idx="selectedIndexFor(item)"
                class="row"
                :class="{ active: isSelectedItem(item), ispinned: item.pinned }"
                @click="copy(item)"
                @mouseenter="selectItem(item)"
              >
                <div class="row-main">
                  <div v-if="kind === 'text'" class="content">{{ item.content }}</div>

                  <div v-else-if="kind === 'image'" class="image-card">
                    <img v-if="item.image_data_url" class="image-preview" :src="item.image_data_url" alt="" />
                    <div class="image-meta-row">
                      <div class="image-caption">{{ imageCaption(item) }}</div>
                      <div class="actions">
                        <button
                          class="act"
                          :class="{ on: item.pinned }"
                          :title="item.pinned ? 'unpin' : 'pin'"
                          @click.stop="pin(item)"
                        >
                          {{ item.pinned ? "⚑" : "⚐" }}
                        </button>
                        <button class="act del" title="delete" @click.stop="remove(item)">
                          ✕
                        </button>
                      </div>
                    </div>
                  </div>

                  <div v-else class="file-list">
                    <div
                      v-for="path in previewFilePaths(item)"
                      :key="`${item.id}-${path}`"
                      class="file-line"
                    >
                      <span class="file-name">{{ displayFileName(path) }}</span>
                      <span class="file-path">{{ path }}</span>
                    </div>
                    <div v-if="item.file_paths.length > 3" class="file-more">
                      +{{ item.file_paths.length - 3 }} more
                    </div>
                  </div>

                  <div class="meta">
                    <span class="tag kind-tag">{{ kindLabel(item) }}</span>
                    <span v-if="item.pinned" class="tag">pinned</span>
                    <span class="bit">{{ relativeTime(item.ts, now) }}</span>
                    <span v-if="primaryMeta(item)" class="bit">{{ primaryMeta(item) }}</span>
                    <span v-if="secondaryMeta(item)" class="bit">{{ secondaryMeta(item) }}</span>
                  </div>
                </div>

                <div v-if="kind !== 'image'" class="actions">
                  <button
                    class="act"
                    :class="{ on: item.pinned }"
                    :title="item.pinned ? 'unpin' : 'pin'"
                    @click.stop="pin(item)"
                  >
                    {{ item.pinned ? "⚑" : "⚐" }}
                  </button>
                  <button class="act del" title="delete" @click.stop="remove(item)">
                    ✕
                  </button>
                </div>
              </article>
              <div v-if="!sectionItems(kind).length" class="section-empty">no {{ scopeColumnTitle(kind) }} clips</div>
            </div>
          </section>
        </template>

        <template v-else-if="scope === 'image' || scope === 'files'">
          <div class="grid-wrap">
            <article
              v-for="item in filtered"
              :key="item.id"
              :data-idx="selectedIndexFor(item)"
              class="row grid-row"
              :class="{ active: isSelectedItem(item), ispinned: item.pinned, 'file-grid-row': item.kind === 'files' }"
              @click="copy(item)"
              @mouseenter="selectItem(item)"
            >
              <div class="row-main">
                <template v-if="item.kind === 'image' && item.image_data_url">
                  <div class="image-card">
                    <img class="image-preview" :src="item.image_data_url" alt="" />
                    <div class="image-meta-row">
                      <div class="image-caption">{{ imageCaption(item) }}</div>
                      <div class="actions">
                        <button
                          class="act"
                          :class="{ on: item.pinned }"
                          :title="item.pinned ? 'unpin' : 'pin'"
                          @click.stop="pin(item)"
                        >
                          {{ item.pinned ? "⚑" : "⚐" }}
                        </button>
                        <button class="act del" title="delete" @click.stop="remove(item)">
                          ✕
                        </button>
                      </div>
                    </div>
                  </div>
                </template>
                <template v-else-if="item.kind === 'files'">
                  <div class="file-list">
                    <div
                      v-for="path in previewFilePaths(item)"
                      :key="`${item.id}-${path}`"
                      class="file-line"
                    >
                      <span class="file-name">{{ displayFileName(path) }}</span>
                      <span class="file-path">{{ path }}</span>
                    </div>
                    <div v-if="item.file_paths.length > 3" class="file-more">
                      +{{ item.file_paths.length - 3 }} more
                    </div>
                  </div>
                </template>
                <div class="meta">
                  <span class="tag kind-tag">{{ kindLabel(item) }}</span>
                  <span v-if="item.pinned" class="tag">pinned</span>
                  <span class="bit">{{ relativeTime(item.ts, now) }}</span>
                  <span v-if="primaryMeta(item)" class="bit">{{ primaryMeta(item) }}</span>
                  <span v-if="secondaryMeta(item)" class="bit">{{ secondaryMeta(item) }}</span>
                </div>
              </div>
              <div v-if="item.kind !== 'image'" class="actions">
                <button
                  class="act"
                  :class="{ on: item.pinned }"
                  :title="item.pinned ? 'unpin' : 'pin'"
                  @click.stop="pin(item)"
                >
                  {{ item.pinned ? "⚑" : "⚐" }}
                </button>
                <button class="act del" title="delete" @click.stop="remove(item)">
                  ✕
                </button>
              </div>
            </article>
          </div>
        </template>

        <template v-else>
          <article
            v-for="item in filtered"
            :key="item.id"
            :data-idx="selectedIndexFor(item)"
            class="row"
            :class="{ active: isSelectedItem(item), ispinned: item.pinned }"
            @click="copy(item)"
            @mouseenter="selectItem(item)"
          >
            <div class="row-main">
              <div class="content">{{ item.content }}</div>
              <div class="meta">
                <span class="tag kind-tag">{{ kindLabel(item) }}</span>
                <span v-if="item.pinned" class="tag">pinned</span>
                <span class="bit">{{ relativeTime(item.ts, now) }}</span>
                <span v-if="primaryMeta(item)" class="bit">{{ primaryMeta(item) }}</span>
                <span v-if="secondaryMeta(item)" class="bit">{{ secondaryMeta(item) }}</span>
              </div>
            </div>
            <div class="actions">
              <button
                class="act"
                :class="{ on: item.pinned }"
                :title="item.pinned ? 'unpin' : 'pin'"
                @click.stop="pin(item)"
              >
                {{ item.pinned ? "⚑" : "⚐" }}
              </button>
              <button class="act del" title="delete" @click.stop="remove(item)">
                ✕
              </button>
            </div>
          </article>
        </template>
      </template>

      <div v-else class="empty">
        <div class="empty-glyph">⌁</div>
        <p>{{ emptyMessage() }}</p>
      </div>
    </main>

    <transition name="toast">
      <div v-if="toast" class="toast">{{ toast }}</div>
    </transition>
  </div>
</template>

<style scoped>
.app {
  display: flex;
  flex-direction: column;
  height: 100%;
  background: var(--bg);
}

/* ---- top bar ---- */
.topbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 10px 14px;
  border-bottom: 1px solid var(--border-soft);
  flex: 0 0 auto;
  background: var(--bg-elev);
}
.module-topbar {
  justify-content: flex-end;
}
.brand {
  display: flex;
  align-items: center;
  font-size: inherit;
}
.logo {
  display: inline-flex;
  width: 18px;
  height: 18px;
  color: #fff;
}
.logo svg {
  display: block;
  width: 100%;
  height: 100%;
}
:root[data-theme="light"] .logo {
  color: var(--accent);
}
.name {
  font-weight: 700;
  letter-spacing: 0.5px;
}
.accent {
  color: var(--accent);
}
.ver {
  color: var(--text-faint);
  font-size: 11px;
}
.stat {
  color: var(--text-dim);
  font-size: 11px;
}
.count {
  color: var(--text);
  font-weight: 600;
}
.pin-stat {
  color: var(--accent-dim);
}
.topright {
  display: flex;
  align-items: center;
  gap: 10px;
}
.theme-btn {
  width: 24px;
  height: 24px;
  display: grid;
  place-items: center;
  background: transparent;
  border: 1px solid var(--border);
  border-radius: 6px;
  color: var(--text-dim);
  font-size: 12px;
  cursor: pointer;
  transition: all 0.08s ease;
}
.theme-btn:hover {
  color: var(--accent);
  border-color: var(--accent-dim);
}

/* ---- search ---- */
.toolbar {
  display: flex;
  align-items: center;
  gap: 10px;
  margin: 18px 18px 0;
}

.searchbar {
  display: flex;
  align-items: center;
  gap: 10px;
  min-height: 38px;
  padding: 9px 11px;
  flex: 1;
  border: 1px solid var(--border);
  border-radius: 10px;
  background: var(--panel);
}
.prompt {
  color: var(--accent);
  font-weight: 700;
  flex: 0 0 auto;
}
.search {
  flex: 1;
  background: transparent;
  border: none;
  outline: none;
  color: var(--text);
  font-family: inherit;
  font-size: inherit;
  line-height: 1.2;
  caret-color: var(--accent);
}
.search::placeholder {
  color: var(--text-faint);
}
.toolbar-actions {
  display: flex;
  align-items: center;
  gap: 6px;
}

.action {
  width: 28px;
  height: 28px;
  display: grid;
  place-items: center;
  padding: 0;
  background: var(--panel);
  border: 1px solid var(--border);
  border-radius: 7px;
  color: var(--text-faint);
  cursor: pointer;
  transition:
    border-color 120ms ease,
    color 120ms ease,
    box-shadow 120ms ease,
    background-color 120ms ease;
}

.action svg {
  width: 14px;
  height: 14px;
}

.action:hover:not(:disabled),
.action:focus-visible {
  color: var(--text);
  border-color: var(--accent-dim);
  box-shadow: 0 0 0 4px var(--accent-glow);
  outline: none;
}

.action.active {
  color: var(--accent);
  border-color: var(--accent-dim);
}

.secondary-action {
  color: var(--text-dim);
}

.danger-action {
  color: var(--danger);
}

.action:disabled {
  opacity: 0.35;
  cursor: not-allowed;
}

/* ---- list ---- */
.list {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  margin: 12px 18px 0;
  padding: 6px;
  border: 1px solid var(--border);
  border-radius: 18px;
  background: var(--panel);
}

.list.split-layout {
  overflow: hidden;
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: 12px;
  padding: 0;
  border: 0;
  border-radius: 0;
  background: transparent;
}

.scope-column {
  min-height: 0;
  display: flex;
  flex-direction: column;
  border: 1px solid var(--border);
  border-radius: 12px;
  background: var(--panel);
  color: var(--text);
}

.scope-column.is-drop-target {
  box-shadow: 0 0 0 2px var(--border) inset;
}

.scope-column-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 10px 12px;
  border-bottom: 1px solid var(--border);
  color: var(--text-dim);
  font-size: 11px;
  text-transform: uppercase;
  letter-spacing: 0.4px;
  cursor: grab;
  user-select: none;
}

.scope-column-head:active {
  cursor: grabbing;
}

.scope-column.is-drop-target .scope-column-head {
  background: var(--bg-elev);
}

.scope-column-list {
  min-height: 0;
  overflow: auto;
  padding: 6px;
}

.list.split-layout .row:hover {
  background: var(--bg-elev);
}

.list.split-layout .row.active {
  background: var(--panel);
  border-color: var(--border);
  border-left-color: var(--accent);
}

.list.split-layout .content,
.list.split-layout .file-name {
  color: var(--text);
}

.list.split-layout .act {
  border-color: var(--border);
}

.list.split-layout .file-line {
  border-color: var(--border-soft);
  background: var(--bg-elev);
}

.list.split-layout .kind-tag {
  border-color: var(--border);
}

.section-empty {
  color: var(--text-dim);
  font-size: 11px;
  text-align: center;
  padding: 20px 8px;
}

.list.split-layout .empty {
  grid-column: 1 / -1;
  width: 100%;
}

.list.grid-layout {
  overflow-y: auto;
}

.grid-wrap {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: 8px;
}
.row {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 9px 10px;
  margin-bottom: 4px;
  border: 1px solid transparent;
  border-left: 2px solid transparent;
  border-radius: 7px;
  cursor: pointer;
  transition: background 0.08s ease, border-color 0.08s ease;
}
.row:hover {
  background: var(--bg-elev);
}
.row.active {
  background: var(--panel);
  border-color: var(--border);
  border-left-color: var(--accent);
  box-shadow: 0 0 0 1px var(--accent-glow);
}

.grid-row {
  margin-bottom: 0;
  height: 100%;
  flex-direction: column;
  align-items: stretch;
  gap: 10px;
}

.grid-row .actions {
  align-self: flex-end;
}

.file-grid-row {
  flex-direction: row;
  align-items: center;
}

.file-grid-row .actions {
  align-self: center;
  margin-left: auto;
}

.grid-row .row-main {
  width: 100%;
}
.row.ispinned {
  border-left-color: var(--accent-dim);
}
.row.ispinned.active {
  border-left-color: var(--accent);
}
.row-main {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.content {
  color: var(--text);
  white-space: pre-wrap;
  word-break: break-word;
  display: -webkit-box;
  -webkit-line-clamp: 3;
  line-clamp: 3;
  -webkit-box-orient: vertical;
  overflow: hidden;
}
.image-card {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.image-meta-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
}

.image-meta-row .actions {
  flex: 0 0 auto;
}
.image-preview {
  display: block;
  max-width: min(100%, 240px);
  max-height: 124px;
  object-fit: contain;
  border-radius: 8px;
  border: 1px solid var(--border);
  background: var(--bg-elev);
}

.grid-row .image-preview {
  width: 100%;
  max-width: none;
  max-height: none;
  height: 170px;
  object-fit: contain;
}
.image-caption {
  color: var(--text-dim);
  font-size: 12px;
}
.file-list {
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.file-line {
  display: flex;
  flex-direction: column;
  gap: 2px;
  padding: 7px 8px;
  border: 1px solid var(--border-soft);
  border-radius: 7px;
  background: var(--bg-elev);
}
.file-name {
  color: var(--text);
  font-size: 12px;
  font-weight: 600;
  word-break: break-word;
}
.file-path {
  color: var(--text-dim);
  font-size: 11px;
  word-break: break-word;
}
.file-more {
  color: var(--text-faint);
  font-size: 11px;
}
.meta {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 8px;
  margin-top: 5px;
  font-size: 10.5px;
  color: var(--text-dim);
}
.bit {
  position: relative;
}
.bit + .bit::before {
  content: "·";
  position: absolute;
  left: -6px;
  color: var(--text-faint);
}
.tag {
  color: var(--accent);
  border: 1px solid var(--accent-dim);
  border-radius: 4px;
  padding: 0 5px;
  font-size: 9.5px;
  text-transform: uppercase;
  letter-spacing: 0.5px;
}
.kind-tag {
  color: var(--text-dim);
  border-color: var(--border);
}

/* ---- row actions ---- */
.actions {
  display: flex;
  align-items: center;
  align-self: center;
  gap: 4px;
  opacity: 0;
  transition: opacity 0.08s ease;
}
.row:hover .actions,
.row.active .actions {
  opacity: 1;
}
.act {
  width: 26px;
  height: 26px;
  display: grid;
  place-items: center;
  background: transparent;
  border: 1px solid var(--border);
  border-radius: 6px;
  color: var(--text-dim);
  font-size: 13px;
  cursor: pointer;
  transition: all 0.08s ease;
}
.act:hover {
  color: var(--accent);
  border-color: var(--accent-dim);
}
.act.on {
  color: var(--accent);
  border-color: var(--accent-dim);
}
.act.del:hover {
  color: var(--danger);
  border-color: var(--danger);
}

/* ---- empty ---- */
.empty {
  height: 100%;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 10px;
  color: var(--text-faint);
  text-align: center;
  padding: 0 30px;
}
.empty-glyph {
  font-size: 34px;
  color: var(--border);
}

/* ---- status bar ---- */
.statusbar {
  display: flex;
  justify-content: flex-end;
  color: var(--text-dim);
  font-size: 12px;
  padding: 2px 2px 0;
}
.hints {
  display: flex;
  flex-wrap: wrap;
  justify-content: flex-end;
  gap: 11px;
  font-size: 10.5px;
  color: var(--text-dim);
}
.hints span {
  display: inline-flex;
  align-items: center;
  gap: 3px;
}
kbd {
  display: inline-block;
  min-width: 15px;
  padding: 0 4px;
  text-align: center;
  background: var(--panel);
  border: 1px solid var(--border);
  border-bottom-width: 2px;
  border-radius: 4px;
  font: inherit;
  font-size: 10px;
  color: var(--text);
}
.history-clear {
  height: 24px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  background: transparent;
  border: 1px solid var(--border);
  color: var(--text-dim);
  border-radius: 6px;
  padding: 0 8px;
  font: inherit;
  font-size: 10.5px;
  cursor: pointer;
  transition: all 0.08s ease;
}
.history-clear:hover:not(:disabled) {
  color: var(--danger);
  border-color: var(--danger);
}
.history-clear:disabled {
  opacity: 0.35;
  cursor: not-allowed;
}

/* ---- toast ---- */
.toast {
  position: fixed;
  left: 50%;
  bottom: 52px;
  transform: translateX(-50%);
  background: var(--panel);
  border: 1px solid var(--accent-dim);
  color: var(--accent);
  padding: 6px 14px;
  border-radius: 8px;
  font-size: 12px;
  box-shadow: 0 8px 24px rgba(0, 0, 0, 0.45), 0 0 0 1px var(--accent-glow);
}
.toast-enter-active,
.toast-leave-active {
  transition: opacity 0.18s ease, transform 0.18s ease;
}
.toast-enter-from,
.toast-leave-to {
  opacity: 0;
  transform: translateX(-50%) translateY(6px);
}

@media (max-width: 1240px) {
  .list.split-layout {
    grid-template-columns: 1fr;
    overflow-y: auto;
  }

  .scope-column {
    min-height: 260px;
  }
}

@media (max-width: 1080px) {
  .grid-wrap {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }
}

@media (max-width: 760px) {
  .grid-wrap {
    grid-template-columns: 1fr;
  }
}
</style>

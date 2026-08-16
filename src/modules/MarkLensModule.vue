<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";
import DOMPurify from "dompurify";
import { marked, type Token } from "marked";

type ThemeMode = "dark" | "light";

interface HeaderMetric {
  value: number | string;
  label: string;
}

const props = withDefaults(defineProps<{ embedded?: boolean; sharedTheme?: ThemeMode }>(), {
  embedded: false,
  sharedTheme: "light",
});

const emit = defineEmits<{
  (e: "header-metrics", metrics: HeaderMetric[]): void;
}>();

interface OutlineItem {
  depth: number;
  id: string;
  label: string;
  prefix: string;
}

const storageKey = "marklens-draft";
const fileNameKey = "marklens-file-name";
const themeKey = "marklens-theme";
const previewOnlyKey = "marklens-preview-only";
const splitRatioKey = "marklens-split-ratio";
const minPanelWidth = 320;
const dividerWidth = 18;
const keyboardResizeStep = 0.03;

marked.setOptions({
  gfm: true,
  breaks: true,
});

const fileInput = ref<HTMLInputElement | null>(null);
const editor = ref<HTMLTextAreaElement | null>(null);
const workspace = ref<HTMLElement | null>(null);
const previewScroll = ref<HTMLElement | null>(null);
const previewArticle = ref<HTMLElement | null>(null);
const content = ref("");
const fileName = ref("Untitled.md");
const theme = ref<ThemeMode>("light");
const previewOnly = ref(false);
const dragActive = ref(false);
const notice = ref("");
const activeOutlineId = ref("");
const outlineOpen = ref(true);
const splitRatio = ref(0.5);
const resizing = ref(false);

let dragDepth = 0;
let noticeTimer: number | undefined;
let ignoreEditorScroll = false;
let ignorePreviewScroll = false;

function collectOutlineItems(tokens: Token[]) {
  const items: OutlineItem[] = [];

  function visit(blocks: Token[]) {
    for (const block of blocks) {
      if (block.type === "heading") {
        const label = block.text.trim() || "untitled section";
        items.push({
          depth: block.depth,
          id: `heading-${items.length + 1}`,
          label,
          prefix: "#".repeat(block.depth),
        });
        continue;
      }

      if (block.type === "blockquote") {
        visit(block.tokens ?? []);
        continue;
      }

      if (block.type === "list") {
        for (const item of block.items) {
          visit(item.tokens ?? []);
        }
      }
    }
  }

  visit(tokens);
  return items;
}

const parsedTokens = computed(() => marked.lexer(content.value) as Token[]);

const rendered = computed(() =>
  DOMPurify.sanitize(marked.parse(content.value) as string, {
    USE_PROFILES: { html: true },
  }),
);

const wordCount = computed(() => {
  const words = content.value.trim().match(/\S+/g);
  return words?.length ?? 0;
});

const charCount = computed(() => content.value.length);

const lineCount = computed(() => content.value.split(/\r?\n/).length);

const outline = computed<OutlineItem[]>(() => collectOutlineItems(parsedTokens.value));

const headingCount = computed(() => outline.value.length);
const headerMetrics = computed<HeaderMetric[]>(() => [
  { value: wordCount.value, label: "words" },
  { value: headingCount.value, label: "heads" },
  { value: lineCount.value, label: "lines" },
]);

const readingMinutes = computed(() => {
  if (!wordCount.value) return 0;
  return Math.max(1, Math.ceil(wordCount.value / 220));
});

const splitPercentage = computed(() => Math.round(splitRatio.value * 100));

const workspaceStyle = computed(() => ({
  "--editor-size": `calc(${splitRatio.value * 100}% - ${dividerWidth / 2}px)`,
}));

function applyTheme(mode: ThemeMode) {
  document.documentElement.dataset.theme = mode;
}

function flash(message: string) {
  notice.value = message;
  window.clearTimeout(noticeTimer);
  noticeTimer = window.setTimeout(() => {
    notice.value = "";
  }, 1400);
}

async function loadMarkdownFile(file: File) {
  const text = await file.text();
  content.value = text;
  fileName.value = file.name;
  flash(`loaded ${file.name}`);
}

function openPicker() {
  fileInput.value?.click();
}

function clearDraft() {
  content.value = "";
  fileName.value = "Untitled.md";
  flash("draft cleared");
}

function togglePreviewOnly() {
  previewOnly.value = !previewOnly.value;
  flash(previewOnly.value ? "preview only" : "split view");
}

function toggleOutline() {
  outlineOpen.value = !outlineOpen.value;
}

function getScrollLimit(element: HTMLElement) {
  return Math.max(0, element.scrollHeight - element.clientHeight);
}

function getScrollProgress(element: HTMLElement) {
  const limit = getScrollLimit(element);
  return limit > 0 ? element.scrollTop / limit : 0;
}

function getElementScrollTop(container: HTMLElement, element: HTMLElement) {
  return (
    element.getBoundingClientRect().top -
    container.getBoundingClientRect().top +
    container.scrollTop
  );
}

function updateActiveOutline() {
  const container = previewScroll.value;
  const article = previewArticle.value;

  if (!container || !article || !outline.value.length) {
    activeOutlineId.value = "";
    return;
  }

  const headings = article.querySelectorAll<HTMLElement>("[data-outline-id]");
  if (!headings.length) {
    activeOutlineId.value = "";
    return;
  }

  const threshold = container.scrollTop + 24;
  let currentId = outline.value[0].id;

  headings.forEach((heading) => {
    const headingId = heading.dataset.outlineId;
    if (!headingId) {
      return;
    }

    if (getElementScrollTop(container, heading) <= threshold) {
      currentId = headingId;
    }
  });

  activeOutlineId.value = currentId;
}

function syncPreviewScrollFromEditor() {
  const source = editor.value;
  const target = previewScroll.value;

  if (!source || !target) {
    return;
  }

  const nextTop = getScrollProgress(source) * getScrollLimit(target);
  if (Math.abs(target.scrollTop - nextTop) < 1) {
    updateActiveOutline();
    return;
  }

  ignorePreviewScroll = true;
  target.scrollTop = nextTop;
  updateActiveOutline();
}

function syncEditorScrollFromPreview() {
  const source = previewScroll.value;
  const target = editor.value;

  if (!source || !target) {
    return;
  }

  const nextTop = getScrollProgress(source) * getScrollLimit(target);
  if (Math.abs(target.scrollTop - nextTop) < 1) {
    return;
  }

  ignoreEditorScroll = true;
  target.scrollTop = nextTop;
}

function onEditorScroll() {
  if (ignoreEditorScroll) {
    ignoreEditorScroll = false;
    return;
  }

  syncPreviewScrollFromEditor();
}

function onPreviewScroll() {
  updateActiveOutline();

  if (ignorePreviewScroll) {
    ignorePreviewScroll = false;
    return;
  }

  syncEditorScrollFromPreview();
}

async function syncPreviewHeadings() {
  await nextTick();

  const article = previewArticle.value;
  if (!article) {
    return;
  }

  const headings = article.querySelectorAll<HTMLHeadingElement>(
    "h1, h2, h3, h4, h5, h6",
  );

  headings.forEach((heading, index) => {
    const item = outline.value[index];
    if (!item) {
      heading.removeAttribute("id");
      delete heading.dataset.outlineId;
      return;
    }

    heading.id = item.id;
    heading.dataset.outlineId = item.id;
  });

  updateActiveOutline();
}

function scrollToHeading(id: string) {
  const container = previewScroll.value;
  const article = previewArticle.value;

  if (!container || !article) {
    return;
  }

  const heading = article.querySelector<HTMLElement>(`[data-outline-id="${id}"]`);
  if (!heading) {
    flash("section unavailable");
    return;
  }

  const targetTop =
    getElementScrollTop(container, heading) - 12;

  activeOutlineId.value = id;

  container.scrollTo({
    top: Math.max(0, targetTop),
    behavior: "smooth",
  });
}

function clampSplitRatio(nextRatio: number) {
  const width = workspace.value?.getBoundingClientRect().width;
  if (!width) {
    return Math.min(0.75, Math.max(0.25, nextRatio));
  }

  const availableWidth = width - dividerWidth;
  if (availableWidth <= minPanelWidth * 2) {
    return 0.5;
  }

  const minRatio = minPanelWidth / availableWidth;
  return Math.min(1 - minRatio, Math.max(minRatio, nextRatio));
}

function updateSplitRatio(clientX: number) {
  const bounds = workspace.value?.getBoundingClientRect();
  if (!bounds) {
    return;
  }

  const availableWidth = bounds.width - dividerWidth;
  if (availableWidth <= 0) {
    return;
  }

  const nextRatio = (clientX - bounds.left - dividerWidth / 2) / availableWidth;
  splitRatio.value = clampSplitRatio(nextRatio);
}

function onWindowPointerMove(event: PointerEvent) {
  if (!resizing.value) {
    return;
  }

  updateSplitRatio(event.clientX);
}

function stopResizing() {
  resizing.value = false;
  document.body.style.cursor = "";
  document.body.style.userSelect = "";
  window.removeEventListener("pointermove", onWindowPointerMove);
  window.removeEventListener("pointerup", stopResizing);
  window.removeEventListener("pointercancel", stopResizing);
}

function onDividerPointerDown(event: PointerEvent) {
  if (previewOnly.value) {
    return;
  }

  event.preventDefault();
  stopResizing();
  resizing.value = true;
  document.body.style.cursor = "col-resize";
  document.body.style.userSelect = "none";
  updateSplitRatio(event.clientX);
  window.addEventListener("pointermove", onWindowPointerMove);
  window.addEventListener("pointerup", stopResizing);
  window.addEventListener("pointercancel", stopResizing);
}

function onDividerKeydown(event: KeyboardEvent) {
  if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") {
    return;
  }

  event.preventDefault();
  const delta = event.key === "ArrowLeft" ? -keyboardResizeStep : keyboardResizeStep;
  splitRatio.value = clampSplitRatio(splitRatio.value + delta);
}

function onWindowResize() {
  splitRatio.value = clampSplitRatio(splitRatio.value);
}

async function copyHtml() {
  if (!navigator.clipboard) {
    flash("clipboard unavailable");
    return;
  }
  await navigator.clipboard.writeText(rendered.value);
  flash("rendered html copied");
}

function onFileChange(event: Event) {
  const input = event.target as HTMLInputElement;
  const file = input.files?.[0];
  if (file) {
    void loadMarkdownFile(file);
  }
  input.value = "";
}

function onDragEnter() {
  dragDepth += 1;
  dragActive.value = true;
}

function onDragLeave() {
  dragDepth = Math.max(0, dragDepth - 1);
  if (dragDepth === 0) {
    dragActive.value = false;
  }
}

function onDrop(event: DragEvent) {
  dragDepth = 0;
  dragActive.value = false;
  const file = event.dataTransfer?.files?.[0];
  if (file) {
    void loadMarkdownFile(file);
  }
}

function onWindowKeydown(event: KeyboardEvent) {
  const mod = event.metaKey || event.ctrlKey;
  if (!mod) return;

  switch (event.key.toLowerCase()) {
    case "o":
      event.preventDefault();
      openPicker();
      break;
    case "k":
      event.preventDefault();
      clearDraft();
      break;
    default:
      break;
  }
}

watch(content, (value) => {
  localStorage.setItem(storageKey, value);
});

watch(fileName, (value) => {
  localStorage.setItem(fileNameKey, value);
});

watch(theme, (value) => {
  applyTheme(value);
  if (!props.embedded) {
    localStorage.setItem(themeKey, value);
  }
});

watch(() => props.sharedTheme, (value) => {
  if (props.embedded && value && theme.value !== value) {
    theme.value = value;
  }
}, { immediate: true });

watch(headerMetrics, (metrics) => {
  emit("header-metrics", metrics);
}, { immediate: true });

watch(previewOnly, (value) => {
  localStorage.setItem(previewOnlyKey, value ? "1" : "0");
  if (value) {
    stopResizing();
    return;
  }

  splitRatio.value = clampSplitRatio(splitRatio.value);
});

watch(splitRatio, (value) => {
  localStorage.setItem(splitRatioKey, String(value));
});

watch([rendered, outline], () => {
  void syncPreviewHeadings();
}, { flush: "post", immediate: true });

watch(rendered, () => {
  void nextTick(() => {
    syncPreviewScrollFromEditor();
  });
}, { flush: "post" });

onMounted(() => {
  const savedContent = localStorage.getItem(storageKey);
  const savedName = localStorage.getItem(fileNameKey);
  const savedTheme = localStorage.getItem(themeKey);
  const savedPreviewOnly = localStorage.getItem(previewOnlyKey);
  const savedSplitRatio = localStorage.getItem(splitRatioKey);

  if (savedContent !== null) {
    content.value = savedContent;
  }

  if (savedName) {
    fileName.value = savedName;
  }

  previewOnly.value = savedPreviewOnly === "1";

  if (savedSplitRatio) {
    const parsedRatio = Number.parseFloat(savedSplitRatio);
    if (Number.isFinite(parsedRatio)) {
      splitRatio.value = clampSplitRatio(parsedRatio);
    }
  }

  if (props.embedded) {
    theme.value = props.sharedTheme;
  } else {
    if (savedTheme === "dark" || savedTheme === "light") {
      theme.value = savedTheme;
    } else if (savedTheme === "night") {
      theme.value = "dark";
    } else if (savedTheme === "paper") {
      theme.value = "light";
    } else {
      theme.value = "light";
    }
  }

  applyTheme(theme.value);
  window.addEventListener("keydown", onWindowKeydown);
  window.addEventListener("resize", onWindowResize);
});

onBeforeUnmount(() => {
  stopResizing();
  window.removeEventListener("keydown", onWindowKeydown);
  window.removeEventListener("resize", onWindowResize);
  window.clearTimeout(noticeTimer);
});
</script>

<template>
  <div
    class="app"
    :class="{ dragging: dragActive, embedded: props.embedded }"
    @dragenter.prevent="onDragEnter"
    @dragover.prevent="dragActive = true"
    @dragleave.prevent="onDragLeave"
    @drop.prevent="onDrop"
  >
    <div class="toolbar">
      <div class="filepill">
        <span class="prompt">#</span>
        <span class="filename" :title="fileName">{{ fileName }}</span>
      </div>

      <div class="toolbar-actions">
        <button class="action icon-action accent-btn" type="button" title="open file" aria-label="open file" @click="openPicker">
          <svg viewBox="0 0 20 20" aria-hidden="true">
            <path d="M4.5 6.5h4l1.4 1.8H15.5a1 1 0 0 1 1 1v4.7a1 1 0 0 1-1 1H4.5a1 1 0 0 1-1-1v-6.7a1 1 0 0 1 1-1Z" fill="none" stroke="currentColor" stroke-linejoin="round" stroke-width="1.5" />
            <path d="M10 10.2v3.4M8.3 11.9H11.7" fill="none" stroke="currentColor" stroke-linecap="round" stroke-width="1.5" />
          </svg>
        </button>
        <button
          class="action icon-action"
          :class="{ on: previewOnly }"
          type="button"
          :title="previewOnly ? 'split view' : 'preview only'"
          :aria-label="previewOnly ? 'split view' : 'preview only'"
          @click="togglePreviewOnly"
        >
          <svg viewBox="0 0 20 20" aria-hidden="true">
            <path d="M4.5 5.5h11a1 1 0 0 1 1 1v7a1 1 0 0 1-1 1h-11a1 1 0 0 1-1-1v-7a1 1 0 0 1 1-1Z" fill="none" stroke="currentColor" stroke-width="1.5" />
            <path d="M10 5.5v9" fill="none" stroke="currentColor" stroke-width="1.5" />
          </svg>
        </button>
        <button class="action icon-action" type="button" title="copy html" aria-label="copy html" @click="copyHtml">
          <svg viewBox="0 0 20 20" aria-hidden="true">
            <rect x="7" y="4.5" width="8.5" height="11" rx="1.5" fill="none" stroke="currentColor" stroke-width="1.5" />
            <path d="M5.5 7V14a1.5 1.5 0 0 0 1.5 1.5" fill="none" stroke="currentColor" stroke-linecap="round" stroke-width="1.5" />
          </svg>
        </button>
        <button class="action icon-action danger-btn" type="button" title="clear draft" aria-label="clear draft" @click="clearDraft">
          <svg viewBox="0 0 20 20" aria-hidden="true">
            <path d="M6 6l8 8M14 6l-8 8" fill="none" stroke="currentColor" stroke-linecap="round" stroke-width="1.7" />
          </svg>
        </button>
      </div>
    </div>

    <main
      ref="workspace"
      class="workspace"
      :class="{ 'preview-only': previewOnly, resizing }"
      :style="workspaceStyle"
    >
      <section class="panel editor-panel">
        <div class="panelbar">
          <span>source</span>
          <span>{{ charCount }} chars</span>
        </div>

        <textarea
          ref="editor"
          v-model="content"
          class="editor"
          spellcheck="false"
          placeholder="Write markdown here, or drop a .md file into the window."
          @scroll="onEditorScroll"
        />
      </section>

      <div
        v-if="!previewOnly"
        class="workspace-divider"
        role="separator"
        tabindex="0"
        aria-label="Resize panels"
        aria-orientation="vertical"
        aria-valuemin="0"
        aria-valuemax="100"
        :aria-valuenow="splitPercentage"
        @pointerdown="onDividerPointerDown"
        @keydown="onDividerKeydown"
      >
        <span class="workspace-divider-handle" aria-hidden="true">
          <span></span>
          <span></span>
          <span></span>
        </span>
      </div>

      <section class="panel preview-panel">
        <div class="panelbar">
          <div class="panelbar-group">
            <span>preview</span>
            <button
              v-if="outline.length"
              class="panelbar-toggle"
              :class="{ on: outlineOpen }"
              type="button"
              :aria-expanded="outlineOpen"
              :aria-label="outlineOpen ? 'collapse outline' : 'expand outline'"
              :title="outlineOpen ? 'collapse outline' : 'expand outline'"
              @click="toggleOutline"
            >
              <svg viewBox="0 0 20 20" aria-hidden="true">
                <path d="M4.5 6h11" fill="none" stroke="currentColor" stroke-linecap="round" stroke-width="1.5" />
                <path d="M4.5 10h11" fill="none" stroke="currentColor" stroke-linecap="round" stroke-width="1.5" />
                <path d="M4.5 14h7" fill="none" stroke="currentColor" stroke-linecap="round" stroke-width="1.5" />
                <path
                  :d="outlineOpen ? 'M14.5 12.5l-2.5-2.5 2.5-2.5' : 'M12 7.5l2.5 2.5-2.5 2.5'"
                  fill="none"
                  stroke="currentColor"
                  stroke-linecap="round"
                  stroke-linejoin="round"
                  stroke-width="1.5"
                />
              </svg>
            </button>
          </div>
          <span>{{ readingMinutes ? `${readingMinutes} min read` : "empty" }}</span>
        </div>

        <section v-if="outline.length && outlineOpen" class="outline-pane">
          <div class="outline-tree">
            <button
              v-for="item in outline"
              :key="item.id"
              class="outline-node"
              :class="{ active: item.id === activeOutlineId }"
              type="button"
              :data-depth="item.depth"
              :style="{ '--depth': String(item.depth - 1) }"
              @click="scrollToHeading(item.id)"
            >
              <span class="outline-prefix">{{ item.prefix }}</span>
              <span class="outline-label">{{ item.label }}</span>
            </button>
          </div>
        </section>

        <div ref="previewScroll" class="preview-scroll" @scroll="onPreviewScroll">
          <article ref="previewArticle" class="prose" v-html="rendered"></article>
        </div>
      </section>
    </main>

    <input
      ref="fileInput"
      class="visually-hidden"
      type="file"
      accept=".md,.markdown,.mdown,.mkdn,.txt,text/markdown,text/plain"
      @change="onFileChange"
    />

    <div v-if="dragActive" class="dropzone">
      <strong>drop markdown</strong>
      <span>replace the current draft</span>
    </div>
  </div>
</template>

<style scoped src="./MarkLensModule.css"></style>
<script setup lang="ts">
import { invoke } from "@tauri-apps/api/core";
import { Menu } from "@tauri-apps/api/menu";
import { getCurrentWindow } from "@tauri-apps/api/window";
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

interface MarkLensTab {
  id: string;
  fileName: string;
  content: string;
  savedPath: string | null;
  lastSavedContent: string;
}

interface MarkdownFilePayload {
  file_name: string;
  path: string;
  content: string;
}

interface SavedMarkdownFile {
  file_name: string;
  path: string;
}

const storageKey = "marklens-draft";
const fileNameKey = "marklens-file-name";
const tabsKey = "marklens-tabs";
const activeTabKey = "marklens-active-tab";
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
const tabList = ref<HTMLElement | null>(null);
const workspace = ref<HTMLElement | null>(null);
const previewScroll = ref<HTMLElement | null>(null);
const previewArticle = ref<HTMLElement | null>(null);
const tabs = ref<MarkLensTab[]>([createTab()]);
const activeTabId = ref(tabs.value[0].id);
const theme = ref<ThemeMode>("light");
const previewOnly = ref(false);
const dragActive = ref(false);
const notice = ref("");
const activeOutlineId = ref("");
const outlineOpen = ref(true);
const splitRatio = ref(0.5);
const resizing = ref(false);
const fullscreenPreview = ref(false);
const pendingCloseTabId = ref<string | null>(null);
const pendingCloseQueue = ref<string[]>([]);
const renamingTabId = ref<string | null>(null);
const renameDraft = ref("");

let dragDepth = 0;
let noticeTimer: number | undefined;
let ignoreEditorScroll = false;
let ignorePreviewScroll = false;

function nextTabId() {
  return `tab-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
}

function createTab(partial: Partial<MarkLensTab> = {}): MarkLensTab {
  return {
    id: partial.id ?? nextTabId(),
    fileName: partial.fileName ?? "Untitled.md",
    content: partial.content ?? "",
    savedPath: partial.savedPath ?? null,
    lastSavedContent: partial.lastSavedContent ?? partial.content ?? "",
  };
}

function isInitialBlankTab() {
  return (
    tabs.value.length === 1
    && tabs.value[0]?.fileName === "Untitled.md"
    && !tabs.value[0]?.content
    && !tabs.value[0]?.savedPath
  );
}

function isTabDirty(tab: MarkLensTab) {
  return tab.content !== tab.lastSavedContent;
}

function nextUntitledName() {
  const used = new Set(tabs.value.map((tab) => tab.fileName));
  if (!used.has("Untitled.md")) {
    return "Untitled.md";
  }

  let index = 2;
  while (used.has(`Untitled-${index}.md`)) {
    index += 1;
  }

  return `Untitled-${index}.md`;
}

const activeTab = computed(() =>
  tabs.value.find((tab) => tab.id === activeTabId.value) ?? tabs.value[0] ?? null,
);

const pendingCloseTab = computed(() =>
  tabs.value.find((tab) => tab.id === pendingCloseTabId.value) ?? null,
);

const content = computed({
  get: () => activeTab.value?.content ?? "",
  set: (value: string) => {
    if (activeTab.value) {
      activeTab.value.content = value;
    }
  },
});

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

function ensureActiveTabVisible() {
  void nextTick(() => {
    tabList.value
      ?.querySelector<HTMLElement>(`[data-tab-id="${activeTabId.value}"]`)
      ?.scrollIntoView({ block: "nearest", inline: "nearest" });
  });
}

function activateTab(id: string) {
  activeTabId.value = id;
  ensureActiveTabVisible();
}

function createEmptyTab() {
  const nextTab = createTab({ fileName: nextUntitledName() });
  tabs.value.push(nextTab);
  activeTabId.value = nextTab.id;
  flash("new tab");
  ensureActiveTabVisible();
}

function toggleFullscreenPreview() {
  fullscreenPreview.value = !fullscreenPreview.value;
}

function queueCloseTabs(tabIds: string[]) {
  const uniqueIds = Array.from(new Set(tabIds));
  if (!uniqueIds.length) {
    return;
  }

  pendingCloseQueue.value = uniqueIds;
  continueClosingTabs();
}

function continueClosingTabs() {
  while (pendingCloseQueue.value.length) {
    const nextId = pendingCloseQueue.value.shift();
    if (!nextId) {
      continue;
    }

    const tab = tabs.value.find((item) => item.id === nextId);
    if (!tab) {
      continue;
    }

    if (isTabDirty(tab)) {
      pendingCloseTabId.value = nextId;
      return;
    }

    finishCloseTab(nextId);
  }
}

async function onTabContextMenu(tab: MarkLensTab, event: MouseEvent) {
  event.preventDefault();
  activateTab(tab.id);
  renamingTabId.value = null;

  const index = tabs.value.findIndex((item) => item.id === tab.id);
  const menu = await Menu.new({
    items: [
      {
        text: "Close Others",
        enabled: tabs.value.length > 1,
        action: () => {
          closeOtherTabs(tab.id);
        },
      },
      {
        text: "Close Left",
        enabled: index > 0,
        action: () => {
          closeTabsToLeft(tab.id);
        },
      },
      {
        text: "Close Right",
        enabled: index >= 0 && index < tabs.value.length - 1,
        action: () => {
          closeTabsToRight(tab.id);
        },
      },
      {
        item: "Separator",
      },
      {
        text: "Copy",
        action: () => {
          duplicateTab(tab.id);
        },
      },
      {
        text: "Delete",
        action: () => {
          closeTab(tab.id);
        },
      },
      {
        text: "Rename",
        action: () => {
          requestRenameTab(tab.id);
        },
      },
    ],
  });

  try {
    await menu.popup(undefined, getCurrentWindow());
  } finally {
    await menu.close();
  }
}

function duplicateTab(tabId: string) {
  const tab = tabs.value.find((item) => item.id === tabId);
  if (!tab) {
    return;
  }

  const duplicate = createTab({
    fileName: tab.fileName,
    content: tab.content,
    savedPath: null,
    lastSavedContent: tab.content,
  });
  tabs.value.push(duplicate);
  activeTabId.value = duplicate.id;
  flash(`copied ${tab.fileName}`);
  ensureActiveTabVisible();
}

function closeOtherTabs(tabId: string) {
  queueCloseTabs(tabs.value.filter((tab) => tab.id !== tabId).map((tab) => tab.id));
}

function closeTabsToLeft(tabId: string) {
  const index = tabs.value.findIndex((tab) => tab.id === tabId);
  if (index <= 0) {
    return;
  }

  queueCloseTabs(tabs.value.slice(0, index).map((tab) => tab.id));
}

function closeTabsToRight(tabId: string) {
  const index = tabs.value.findIndex((tab) => tab.id === tabId);
  if (index < 0 || index >= tabs.value.length - 1) {
    return;
  }

  queueCloseTabs(tabs.value.slice(index + 1).map((tab) => tab.id));
}

function requestRenameTab(tabId: string) {
  const tab = tabs.value.find((item) => item.id === tabId);
  if (!tab) {
    return;
  }

  renamingTabId.value = tab.id;
  renameDraft.value = tab.fileName;
  void nextTick(() => {
    document.querySelector<HTMLInputElement>(`[data-rename-input="${tab.id}"]`)?.focus();
    document.querySelector<HTMLInputElement>(`[data-rename-input="${tab.id}"]`)?.select();
  });
}

async function commitRenameTab(tabId: string) {
  const tab = tabs.value.find((item) => item.id === tabId);
  const trimmed = renameDraft.value.trim();
  if (!tab) {
    renamingTabId.value = null;
    renameDraft.value = "";
    return;
  }

  if (trimmed && trimmed !== tab.fileName) {
    if (tab.savedPath) {
      try {
        const renamed = await invoke<SavedMarkdownFile>("rename_markdown_file", {
          existingPath: tab.savedPath,
          nextFileName: trimmed,
        });
        tab.fileName = renamed.file_name;
        tab.savedPath = renamed.path;
        flash(`renamed to ${renamed.file_name}`);
      } catch {
        flash("rename failed");
        renamingTabId.value = null;
        renameDraft.value = "";
        return;
      }
    } else {
      tab.fileName = trimmed;
      flash(`renamed to ${trimmed}`);
    }
  }

  renamingTabId.value = null;
  renameDraft.value = "";
}

function cancelRenameTab() {
  renamingTabId.value = null;
  renameDraft.value = "";
}

function onRenameKeydown(tabId: string, event: KeyboardEvent) {
  if (event.key === "Enter") {
    event.preventDefault();
    void commitRenameTab(tabId);
  } else if (event.key === "Escape") {
    event.preventDefault();
    cancelRenameTab();
  }
}

function finishCloseTab(id: string) {
  const index = tabs.value.findIndex((tab) => tab.id === id);
  if (index < 0) {
    return;
  }

  const wasActive = activeTabId.value === id;
  tabs.value.splice(index, 1);

  if (!tabs.value.length) {
    const nextTab = createTab();
    tabs.value = [nextTab];
    activeTabId.value = nextTab.id;
    return;
  }

  if (wasActive) {
    activeTabId.value = tabs.value[Math.max(0, index - 1)]?.id ?? tabs.value[0].id;
  }

  ensureActiveTabVisible();
}

function closeTab(id: string) {
  queueCloseTabs([id]);
}

function mergeTabs(nextTabs: MarkLensTab[]) {
  if (!nextTabs.length) {
    return;
  }

  const existing = isInitialBlankTab() ? [] : [...tabs.value];
  let lastActiveId = nextTabs[nextTabs.length - 1].id;

  nextTabs.forEach((nextTab) => {
    const duplicate = nextTab.savedPath
      ? existing.find((tab) => tab.savedPath === nextTab.savedPath)
      : null;

    if (duplicate) {
      duplicate.content = nextTab.content;
      duplicate.fileName = nextTab.fileName;
      duplicate.lastSavedContent = nextTab.lastSavedContent;
      lastActiveId = duplicate.id;
      return;
    }

    existing.push(nextTab);
    lastActiveId = nextTab.id;
  });

  tabs.value = existing.length ? existing : [createTab()];
  activeTabId.value = lastActiveId;
  ensureActiveTabVisible();
}

async function loadMarkdownFiles(files: File[]) {
  if (!files.length) {
    return;
  }

  const openedTabs = await Promise.all(files.map(async (file) => createTab({
    fileName: file.name,
    content: await file.text(),
    lastSavedContent: await file.text(),
  })));
  mergeTabs(openedTabs);
  flash(`opened ${openedTabs.length} file${openedTabs.length === 1 ? "" : "s"}`);
}

async function openPicker() {
  try {
    const payload = await invoke<MarkdownFilePayload[]>("open_markdown_files");
    const openedTabs = payload.map((item) => createTab({
      fileName: item.file_name,
      content: item.content,
      savedPath: item.path,
      lastSavedContent: item.content,
    }));
    mergeTabs(openedTabs);
    if (openedTabs.length) {
      flash(`opened ${openedTabs.length} file${openedTabs.length === 1 ? "" : "s"}`);
    }
    return;
  } catch {
    fileInput.value?.click();
  }
}

function clearDraft() {
  if (!activeTab.value) {
    return;
  }

  activeTab.value.content = "";
  activeTab.value.fileName = "Untitled.md";
  activeTab.value.savedPath = null;
  flash("tab cleared");
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

async function saveTab(tab: MarkLensTab) {
  try {
    const saved = await invoke<SavedMarkdownFile>("save_markdown_file", {
      existingPath: tab.savedPath,
      suggestedName: tab.fileName,
      content: tab.content,
    });
    tab.fileName = saved.file_name;
    tab.savedPath = saved.path;
    tab.lastSavedContent = tab.content;
    flash(`saved ${saved.file_name}`);
    return true;
  } catch (error) {
    if (String(error).includes("save canceled")) {
      flash("save canceled");
      return false;
    }

    if (activeTab.value?.id !== tab.id) {
      activeTabId.value = tab.id;
    }

    downloadCurrentTab();
    tab.lastSavedContent = tab.content;
    flash(`downloaded ${tab.fileName}`);
    return true;
  }
}

function downloadCurrentTab() {
  if (!activeTab.value) {
    return;
  }

  const blob = new Blob([activeTab.value.content], { type: "text/markdown;charset=utf-8" });
  const url = URL.createObjectURL(blob);
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = activeTab.value.fileName;
  anchor.click();
  URL.revokeObjectURL(url);
}

async function saveCurrentTab() {
  if (!activeTab.value) {
    return;
  }

  await saveTab(activeTab.value);
}

function cancelPendingClose() {
  pendingCloseTabId.value = null;
  pendingCloseQueue.value = [];
  flash("close canceled");
}

function discardPendingClose() {
  if (!pendingCloseTabId.value) {
    return;
  }

  const tabId = pendingCloseTabId.value;
  pendingCloseTabId.value = null;
  finishCloseTab(tabId);
  continueClosingTabs();
}

async function saveAndClosePendingTab() {
  const tab = pendingCloseTab.value;
  if (!tab) {
    return;
  }

  const saved = await saveTab(tab);
  if (!saved) {
    return;
  }

  const tabId = tab.id;
  pendingCloseTabId.value = null;
  finishCloseTab(tabId);
  continueClosingTabs();
}

function onFileChange(event: Event) {
  const input = event.target as HTMLInputElement;
  const files = Array.from(input.files ?? []);
  if (files.length) {
    void loadMarkdownFiles(files);
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
  const files = Array.from(event.dataTransfer?.files ?? []);
  if (files.length) {
    void loadMarkdownFiles(files);
  }
}

function onWindowKeydown(event: KeyboardEvent) {
  if (event.key === "Escape" && fullscreenPreview.value) {
    event.preventDefault();
    fullscreenPreview.value = false;
    return;
  }

  if (event.key === "Escape" && pendingCloseTabId.value) {
    event.preventDefault();
    cancelPendingClose();
    return;
  }

  if (event.key === "Escape" && renamingTabId.value) {
    event.preventDefault();
    cancelRenameTab();
    return;
  }

  const mod = event.metaKey || event.ctrlKey;
  if (!mod) return;

  switch (event.key.toLowerCase()) {
    case "o":
      event.preventDefault();
      void openPicker();
      break;
    case "s":
      event.preventDefault();
      void saveCurrentTab();
      break;
    case "k":
      event.preventDefault();
      clearDraft();
      break;
    default:
      break;
  }
}

watch(tabs, (value) => {
  localStorage.setItem(tabsKey, JSON.stringify(value));
}, { deep: true });

watch(activeTabId, (value) => {
  localStorage.setItem(activeTabKey, value);
  ensureActiveTabVisible();
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
  const savedTabs = localStorage.getItem(tabsKey);
  const savedActiveTab = localStorage.getItem(activeTabKey);
  const savedContent = localStorage.getItem(storageKey);
  const savedName = localStorage.getItem(fileNameKey);
  const savedTheme = localStorage.getItem(themeKey);
  const savedPreviewOnly = localStorage.getItem(previewOnlyKey);
  const savedSplitRatio = localStorage.getItem(splitRatioKey);

  if (savedTabs) {
    try {
      const parsed = JSON.parse(savedTabs);
      if (Array.isArray(parsed)) {
        const restoredTabs = parsed
          .filter((item) => item && typeof item.fileName === "string" && typeof item.content === "string")
          .map((item) => createTab({
            id: typeof item.id === "string" ? item.id : undefined,
            fileName: item.fileName,
            content: item.content,
            savedPath: typeof item.savedPath === "string" ? item.savedPath : null,
            lastSavedContent: typeof item.lastSavedContent === "string"
              ? item.lastSavedContent
              : typeof item.savedPath === "string"
                ? item.content
                : "",
          }));

        if (restoredTabs.length) {
          tabs.value = restoredTabs;
        }
      }
    } catch {
      tabs.value = [createTab()];
    }
  } else {
    tabs.value = [createTab({
      fileName: savedName || "Untitled.md",
      content: savedContent ?? "",
      lastSavedContent: "",
    })];
  }

  if (savedActiveTab && tabs.value.some((tab) => tab.id === savedActiveTab)) {
    activeTabId.value = savedActiveTab;
  } else {
    activeTabId.value = tabs.value[0]?.id ?? createTab().id;
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
        <div ref="tabList" class="filepill-tabs" role="tablist" aria-label="open markdown tabs">
          <div
            v-for="tab in tabs"
            :key="tab.id"
            class="file-tab"
            :class="{ active: tab.id === activeTabId, dirty: isTabDirty(tab) }"
            :data-tab-id="tab.id"
            @contextmenu="onTabContextMenu(tab, $event)"
          >
            <button
              v-if="renamingTabId !== tab.id"
              class="file-tab-button"
              type="button"
              role="tab"
              :aria-selected="tab.id === activeTabId"
              :title="tab.savedPath ?? tab.fileName"
              @click="activateTab(tab.id)"
            >
              <span class="file-tab-label">{{ tab.fileName }}</span>
              <span v-if="isTabDirty(tab)" class="file-tab-dirty-slot" aria-hidden="true"></span>
            </button>
            <input
              v-else
              :data-rename-input="tab.id"
              v-model="renameDraft"
              class="file-tab-rename-input"
              type="text"
              spellcheck="false"
              @blur="commitRenameTab(tab.id)"
              @keydown="onRenameKeydown(tab.id, $event)"
            />
            <div class="file-tab-accessory">
              <span v-if="isTabDirty(tab)" class="file-tab-dirty-dot" aria-hidden="true"></span>
              <button
                v-if="tabs.length > 1"
                class="file-tab-close"
                type="button"
                :title="`close ${tab.fileName}`"
                :aria-label="`close ${tab.fileName}`"
                @click.stop="closeTab(tab.id)"
              >
                <span class="file-tab-close-glyph" aria-hidden="true">×</span>
              </button>
            </div>
          </div>
        </div>
      </div>

      <div class="toolbar-actions">
        <button class="action icon-action accent-btn" type="button" title="open files" aria-label="open files" @click="openPicker">
          <svg viewBox="0 0 20 20" aria-hidden="true">
            <path d="M4.5 6.5h4l1.4 1.8H15.5a1 1 0 0 1 1 1v4.7a1 1 0 0 1-1 1H4.5a1 1 0 0 1-1-1v-6.7a1 1 0 0 1 1-1Z" fill="none" stroke="currentColor" stroke-linejoin="round" stroke-width="1.5" />
            <path d="M10 10.2v3.4M8.3 11.9H11.7" fill="none" stroke="currentColor" stroke-linecap="round" stroke-width="1.5" />
          </svg>
        </button>
        <button class="action icon-action" type="button" title="new tab" aria-label="new tab" @click="createEmptyTab">
          <svg viewBox="0 0 20 20" aria-hidden="true">
            <path d="M5.5 4.5h7.8l1.7 1.7v9.3a1 1 0 0 1-1 1h-8.5a1 1 0 0 1-1-1v-10a1 1 0 0 1 1-1Z" fill="none" stroke="currentColor" stroke-linejoin="round" stroke-width="1.5" />
            <path d="M10 8v5M7.5 10.5h5" fill="none" stroke="currentColor" stroke-linecap="round" stroke-width="1.5" />
          </svg>
        </button>
        <button class="action icon-action" type="button" title="save file" aria-label="save file" @click="saveCurrentTab">
          <svg viewBox="0 0 20 20" aria-hidden="true">
            <path d="M5.5 4.5h7.8l1.7 1.7v9.3a1 1 0 0 1-1 1h-8.5a1 1 0 0 1-1-1v-10a1 1 0 0 1 1-1Z" fill="none" stroke="currentColor" stroke-linejoin="round" stroke-width="1.5" />
            <path d="M7.5 4.5v4h5v-4" fill="none" stroke="currentColor" stroke-linejoin="round" stroke-width="1.5" />
            <path d="M7.5 13h5" fill="none" stroke="currentColor" stroke-linecap="round" stroke-width="1.5" />
          </svg>
        </button>
        <button
          class="action icon-action"
          :class="{ on: fullscreenPreview }"
          type="button"
          :title="fullscreenPreview ? 'exit fullscreen preview' : 'fullscreen preview'"
          :aria-label="fullscreenPreview ? 'exit fullscreen preview' : 'fullscreen preview'"
          @click="toggleFullscreenPreview"
        >
          <svg viewBox="0 0 20 20" aria-hidden="true">
            <path d="M4.5 8V5.5h2.5M15.5 8V5.5H13M4.5 12v2.5h2.5M15.5 12v2.5H13" fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" stroke-width="1.5" />
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
      multiple
      accept=".md,.markdown,.mdown,.mkdn,.txt,text/markdown,text/plain"
      @change="onFileChange"
    />

    <div v-if="dragActive" class="dropzone">
      <strong>drop markdown</strong>
      <span>open in a new tab</span>
    </div>

    <div v-if="fullscreenPreview" class="fullscreen-preview">
      <div class="fullscreen-preview-bar">
        <span class="fullscreen-preview-title">{{ activeTab?.fileName ?? 'Preview' }}</span>
        <button
          class="action icon-action"
          type="button"
          title="exit fullscreen preview"
          aria-label="exit fullscreen preview"
          @click="toggleFullscreenPreview"
        >
          <svg viewBox="0 0 20 20" aria-hidden="true">
            <path d="M6 6l8 8M14 6l-8 8" fill="none" stroke="currentColor" stroke-linecap="round" stroke-width="1.7" />
          </svg>
        </button>
      </div>
      <div class="fullscreen-preview-scroll">
        <article class="prose" v-html="rendered"></article>
      </div>
    </div>

    <div v-if="pendingCloseTab" class="confirm-backdrop" @click.self="cancelPendingClose">
      <div class="confirm-dialog" role="dialog" aria-modal="true" aria-label="Unsaved changes">
        <strong>Unsaved Changes</strong>
        <p>
          Save changes to <span class="confirm-file-name">{{ pendingCloseTab.fileName }}</span> before closing?
        </p>
        <div class="confirm-actions">
          <button class="action secondary-dialog-action" type="button" @click="cancelPendingClose">
            Cancel
          </button>
          <button class="action danger-dialog-action" type="button" @click="discardPendingClose">
            Don’t Save
          </button>
          <button class="action accent-btn dialog-save-action" type="button" @click="saveAndClosePendingTab">
            Save
          </button>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped src="./MarkLensModule.css"></style>
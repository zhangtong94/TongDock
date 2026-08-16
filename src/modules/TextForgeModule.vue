<script setup lang="ts">
import { format as formatSql } from "sql-formatter";
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";

type ThemeMode = "dark" | "light";

interface HeaderMetric {
  value: number | string;
  label: string;
}

const props = withDefaults(defineProps<{ embedded?: boolean; sharedTheme?: ThemeMode }>(), {
  embedded: false,
  sharedTheme: "dark",
});

const emit = defineEmits<{
  (e: "header-metrics", metrics: HeaderMetric[]): void;
}>();

type ToolGroupId = "encoding" | "formatting" | "sql";
type ToolId =
  | "base64-encode"
  | "base64-decode"
  | "url-encode"
  | "url-decode"
  | "html-escape"
  | "html-unescape"
  | "unicode-escape"
  | "unicode-unescape"
  | "json-pretty"
  | "json-minify"
  | "line-endings-lf"
  | "line-endings-crlf"
  | "trim-line-edges"
  | "tabs-to-2-spaces"
  | "tabs-to-4-spaces"
  | "leading-2-spaces-to-tabs"
  | "leading-4-spaces-to-tabs"
  | "sql-format"
  | "sql-string-literal"
  | "sql-quoted-in"
  | "sql-numeric-in"
  | "sql-columns"
  | "sql-assignments";

interface ToolGroup {
  id: ToolGroupId;
  name: string;
  shortName: string;
  note: string;
}

interface ToolDefinition {
  id: ToolId;
  group: ToolGroupId;
  label: string;
  summary: string;
  inputHint: string;
  outputHint: string;
  run: (input: string) => string;
}

interface ToolGroupView extends ToolGroup {
  tools: ToolDefinition[];
}

const draftKey = "textforge-draft";
const themeKey = "textforge-theme";
const toolKey = "textforge-tool";

const groups: ToolGroup[] = [
  {
    id: "encoding",
    name: "encoding conversion",
    shortName: "encoding",
    note: "portable encodings for URLs, payloads, entities, and escaped strings",
  },
  {
    id: "formatting",
    name: "format organization",
    shortName: "format",
    note: "normalize structure, indentation, whitespace, and structured JSON output",
  },
  {
    id: "sql",
    name: "sql helpers",
    shortName: "sql",
    note: "clean queries and turn pasted values or columns into SQL-ready fragments",
  },
];

const utf8Encoder = new TextEncoder();
const utf8Decoder = new TextDecoder("utf-8", { fatal: true });

function bytesToBase64(bytes: Uint8Array): string {
  let binary = "";
  const chunkSize = 0x8000;

  for (let index = 0; index < bytes.length; index += chunkSize) {
    binary += String.fromCharCode(...bytes.subarray(index, index + chunkSize));
  }

  return btoa(binary);
}

function base64ToBytes(value: string): Uint8Array {
  const compact = value.replace(/\s+/g, "");
  const binary = atob(compact);
  const bytes = new Uint8Array(binary.length);

  for (let index = 0; index < binary.length; index += 1) {
    bytes[index] = binary.charCodeAt(index);
  }

  return bytes;
}

function htmlEscape(value: string): string {
  const node = document.createElement("div");
  node.textContent = value;
  return node.innerHTML;
}

function htmlUnescape(value: string): string {
  const node = document.createElement("textarea");
  node.innerHTML = value;
  return node.value;
}

function unicodeEscape(value: string): string {
  let output = "";

  for (const char of value) {
    const codePoint = char.codePointAt(0) ?? 0;

    if (char === "\\") {
      output += "\\\\";
      continue;
    }

    if (char === "\n") {
      output += "\\n";
      continue;
    }

    if (char === "\r") {
      output += "\\r";
      continue;
    }

    if (char === "\t") {
      output += "\\t";
      continue;
    }

    if (/^[ -~]$/.test(char)) {
      output += char;
      continue;
    }

    if (codePoint <= 0xffff) {
      output += `\\u${codePoint.toString(16).padStart(4, "0")}`;
      continue;
    }

    const offset = codePoint - 0x10000;
    const high = 0xd800 + (offset >> 10);
    const low = 0xdc00 + (offset & 0x3ff);
    output += `\\u${high.toString(16).padStart(4, "0")}\\u${low.toString(16).padStart(4, "0")}`;
  }

  return output;
}

function unicodeUnescape(value: string): string {
  let output = "";

  for (let index = 0; index < value.length; index += 1) {
    const char = value[index];

    if (char !== "\\") {
      output += char;
      continue;
    }

    const next = value[index + 1];
    if (!next) {
      throw new Error("trailing backslash in escaped text");
    }

    if (next === "u") {
      const hex = value.slice(index + 2, index + 6);
      if (!/^[0-9a-fA-F]{4}$/.test(hex)) {
        throw new Error("invalid unicode escape sequence");
      }

      output += String.fromCharCode(Number.parseInt(hex, 16));
      index += 5;
      continue;
    }

    const simpleEscapes: Record<string, string> = {
      "\\": "\\",
      n: "\n",
      r: "\r",
      t: "\t",
      b: "\b",
      f: "\f",
      '"': '"',
      "'": "'",
    };

    if (!(next in simpleEscapes)) {
      throw new Error(`unsupported escape sequence \\${next}`);
    }

    output += simpleEscapes[next];
    index += 1;
  }

  return output;
}

function normalizeToLf(value: string): string {
  return value.replace(/\r\n|\r/g, "\n");
}

function normalizeToCrlf(value: string): string {
  return normalizeToLf(value).replace(/\n/g, "\r\n");
}

function trimLineEdges(value: string): string {
  return value.replace(/^[ \t]+|[ \t]+$/gm, "");
}

function tabsToSpaces(value: string, size: number): string {
  return value.replace(/\t/g, " ".repeat(size));
}

function leadingSpacesToTabs(value: string, size: number): string {
  const pattern = new RegExp(`^((?: {${size}})+)`, "gm");
  return value.replace(pattern, (match) => "\t".repeat(match.length / size));
}

function splitListItems(value: string): string[] {
  return normalizeToLf(value)
    .split("\n")
    .map((item) => item.trim())
    .filter(Boolean)
    .map((item) => item.replace(/,+$/g, ""));
}

function toSqlLiteral(value: string): string {
  return `'${value.replace(/'/g, "''")}'`;
}

function formatGenericSql(value: string): string {
  return formatSql(value, {
    language: "sql",
    keywordCase: "upper",
    linesBetweenQueries: 1,
    tabWidth: 2,
  });
}

function linesToQuotedIn(value: string): string {
  const items = splitListItems(value);
  return `(${items.map((item) => toSqlLiteral(item)).join(", ")})`;
}

function linesToNumericIn(value: string): string {
  const items = splitListItems(value);
  const invalid = items.find((item) => !/^-?\d+(?:\.\d+)?$/.test(item));

  if (invalid) {
    throw new Error(`non-numeric value: ${invalid}`);
  }

  return `(${items.join(", ")})`;
}

function linesToColumns(value: string): string {
  return splitListItems(value).join(",\n");
}

function linesToAssignments(value: string): string {
  return splitListItems(value)
    .map((item) => `${item} = ?`)
    .join(",\n");
}

function countLines(value: string): number {
  return value ? value.split(/\r\n|\n|\r/).length : 0;
}

const tools: ToolDefinition[] = [
  {
    id: "base64-encode",
    group: "encoding",
    label: "base64 encode",
    summary: "turn UTF-8 text into a portable Base64 payload",
    inputHint: "Paste raw text, JSON, or credentials-safe payloads here.",
    outputHint: "Encoded Base64 output appears here.",
    run: (input) => bytesToBase64(utf8Encoder.encode(input)),
  },
  {
    id: "base64-decode",
    group: "encoding",
    label: "base64 decode",
    summary: "decode a Base64 payload back into UTF-8 text",
    inputHint: "Paste a Base64 string here.",
    outputHint: "Decoded UTF-8 text appears here.",
    run: (input) => utf8Decoder.decode(base64ToBytes(input)),
  },
  {
    id: "url-encode",
    group: "encoding",
    label: "url encode",
    summary: "escape characters for query strings and URL-safe fragments",
    inputHint: "Paste text that needs URL-safe encoding.",
    outputHint: "Percent-encoded output appears here.",
    run: (input) => encodeURIComponent(input),
  },
  {
    id: "url-decode",
    group: "encoding",
    label: "url decode",
    summary: "decode percent-escaped text back into readable characters",
    inputHint: "Paste a percent-encoded string here.",
    outputHint: "Decoded URL text appears here.",
    run: (input) => decodeURIComponent(input),
  },
  {
    id: "html-escape",
    group: "encoding",
    label: "html escape",
    summary: "escape markup-sensitive characters into HTML entities",
    inputHint: "Paste raw HTML or mixed content here.",
    outputHint: "Escaped entity output appears here.",
    run: htmlEscape,
  },
  {
    id: "html-unescape",
    group: "encoding",
    label: "html unescape",
    summary: "restore HTML entities back into literal characters",
    inputHint: "Paste HTML entities such as &amp;lt; or &amp;amp;.",
    outputHint: "Literal unescaped text appears here.",
    run: htmlUnescape,
  },
  {
    id: "unicode-escape",
    group: "encoding",
    label: "unicode escape",
    summary: "convert non-ASCII and control characters into escaped sequences",
    inputHint: "Paste human-readable text here.",
    outputHint: "Escaped unicode output appears here.",
    run: unicodeEscape,
  },
  {
    id: "unicode-unescape",
    group: "encoding",
    label: "unicode unescape",
    summary: "turn escaped sequences such as \\u4f60 back into text",
    inputHint: "Paste escaped text such as \\u4f60\\u597d or \\n.",
    outputHint: "Decoded unicode text appears here.",
    run: unicodeUnescape,
  },
  {
    id: "json-pretty",
    group: "formatting",
    label: "json pretty",
    summary: "parse JSON and rewrite it with readable indentation",
    inputHint: "Paste valid JSON here.",
    outputHint: "Pretty-printed JSON appears here.",
    run: (input) => JSON.stringify(JSON.parse(input), null, 2),
  },
  {
    id: "json-minify",
    group: "formatting",
    label: "json minify",
    summary: "strip whitespace from JSON while keeping the structure intact",
    inputHint: "Paste valid JSON here.",
    outputHint: "Minified JSON appears here.",
    run: (input) => JSON.stringify(JSON.parse(input)),
  },
  {
    id: "line-endings-lf",
    group: "formatting",
    label: "line endings -> lf",
    summary: "normalize mixed line endings to Unix-style LF",
    inputHint: "Paste text with Windows or mixed line endings here.",
    outputHint: "LF-normalized output appears here.",
    run: normalizeToLf,
  },
  {
    id: "line-endings-crlf",
    group: "formatting",
    label: "line endings -> crlf",
    summary: "normalize line endings to Windows-style CRLF",
    inputHint: "Paste text that should use CRLF endings.",
    outputHint: "CRLF-normalized output appears here.",
    run: normalizeToCrlf,
  },
  {
    id: "trim-line-edges",
    group: "formatting",
    label: "trim line edges",
    summary: "remove leading and trailing spaces from every line",
    inputHint: "Paste text with uneven indentation or trailing spaces.",
    outputHint: "Trimmed line output appears here.",
    run: trimLineEdges,
  },
  {
    id: "tabs-to-2-spaces",
    group: "formatting",
    label: "tabs -> 2 spaces",
    summary: "expand every tab into two spaces",
    inputHint: "Paste tab-indented text here.",
    outputHint: "Space-indented output appears here.",
    run: (input) => tabsToSpaces(input, 2),
  },
  {
    id: "tabs-to-4-spaces",
    group: "formatting",
    label: "tabs -> 4 spaces",
    summary: "expand every tab into four spaces",
    inputHint: "Paste tab-indented text here.",
    outputHint: "Space-indented output appears here.",
    run: (input) => tabsToSpaces(input, 4),
  },
  {
    id: "leading-2-spaces-to-tabs",
    group: "formatting",
    label: "leading 2 spaces -> tabs",
    summary: "collapse leading two-space indentation back into tabs",
    inputHint: "Paste text with leading two-space indentation.",
    outputHint: "Tab-indented output appears here.",
    run: (input) => leadingSpacesToTabs(input, 2),
  },
  {
    id: "leading-4-spaces-to-tabs",
    group: "formatting",
    label: "leading 4 spaces -> tabs",
    summary: "collapse leading four-space indentation back into tabs",
    inputHint: "Paste text with leading four-space indentation.",
    outputHint: "Tab-indented output appears here.",
    run: (input) => leadingSpacesToTabs(input, 4),
  },
  {
    id: "sql-format",
    group: "sql",
    label: "sql format",
    summary: "reflow a query into readable SQL with uppercase keywords",
    inputHint: "Paste a SQL query or multiple statements here.",
    outputHint: "Formatted SQL appears here.",
    run: formatGenericSql,
  },
  {
    id: "sql-string-literal",
    group: "sql",
    label: "text -> sql literal",
    summary: "escape single quotes and wrap the current text as one SQL string literal",
    inputHint: "Paste raw text that should become a quoted SQL literal.",
    outputHint: "A safely quoted SQL string literal appears here.",
    run: toSqlLiteral,
  },
  {
    id: "sql-quoted-in",
    group: "sql",
    label: "lines -> quoted in",
    summary: "turn one value per line into a quoted SQL IN list",
    inputHint: "Paste one string value per line.",
    outputHint: "A quoted SQL IN list appears here.",
    run: linesToQuotedIn,
  },
  {
    id: "sql-numeric-in",
    group: "sql",
    label: "lines -> numeric in",
    summary: "turn one numeric value per line into a numeric SQL IN list",
    inputHint: "Paste one integer or decimal value per line.",
    outputHint: "A numeric SQL IN list appears here.",
    run: linesToNumericIn,
  },
  {
    id: "sql-columns",
    group: "sql",
    label: "lines -> columns",
    summary: "turn a column list into comma-separated SQL select fields",
    inputHint: "Paste one column name per line.",
    outputHint: "A comma-separated SQL column list appears here.",
    run: linesToColumns,
  },
  {
    id: "sql-assignments",
    group: "sql",
    label: "lines -> assignments",
    summary: "turn column names into assignment lines like column = ?",
    inputHint: "Paste one column name per line.",
    outputHint: "An update-friendly assignment list appears here.",
    run: linesToAssignments,
  },
];

const toolMap = new Map<ToolId, ToolDefinition>(tools.map((tool) => [tool.id, tool]));

const inputEl = ref<HTMLTextAreaElement | null>(null);
const input = ref("");
const theme = ref<ThemeMode>("dark");
const selectedToolId = ref<ToolId>("base64-encode");
const notice = ref("");

let noticeTimer: number | undefined;

const groupedTools = computed<ToolGroupView[]>(() =>
  groups.map((group) => {
    const groupTools = tools.filter((tool) => tool.group === group.id);
    return {
      ...group,
      tools: groupTools,
    };
  }),
);

const selectedTool = computed(() => toolMap.get(selectedToolId.value)!);

const selectedGroup = computed(
  () => groups.find((group) => group.id === selectedTool.value.group) ?? groups[0],
);

const currentGroupView = computed(
  () => groupedTools.value.find((group) => group.id === selectedGroup.value.id) ?? groupedTools.value[0],
);

const result = computed(() => {
  if (!input.value) {
    return { output: "", error: "" };
  }

  try {
    return {
      output: selectedTool.value.run(input.value),
      error: "",
    };
  } catch (error) {
    return {
      output: "",
      error: error instanceof Error ? error.message : "transform failed",
    };
  }
});

const inputCharCount = computed(() => input.value.length);
const inputLineCount = computed(() => countLines(input.value));
const outputCharCount = computed(() => result.value.output.length);
const outputLineCount = computed(() => countLines(result.value.output));
const canUseOutput = computed(() => input.value.length > 0 && !result.value.error);
const headerMetrics = computed<HeaderMetric[]>(() => [
  { value: inputCharCount.value, label: "in" },
  { value: outputCharCount.value, label: "out" },
]);

function applyTheme(mode: ThemeMode) {
  document.documentElement.dataset.theme = mode;
}

function flash(message: string) {
  notice.value = message;
  window.clearTimeout(noticeTimer);
  noticeTimer = window.setTimeout(() => {
    notice.value = "";
  }, 1500);
}

function setTool(id: ToolId) {
  selectedToolId.value = id;
}

function setGroup(id: ToolGroupId) {
  if (selectedGroup.value.id === id) {
    return;
  }

  const firstTool = tools.find((tool) => tool.group === id);
  if (firstTool) {
    selectedToolId.value = firstTool.id;
  }
}

async function pasteInput() {
  if (!navigator.clipboard) {
    flash("clipboard api unavailable");
    return;
  }

  try {
    input.value = await navigator.clipboard.readText();
    flash("input pasted from clipboard");
  } catch {
    flash("clipboard paste blocked");
  }
}

async function copyOutput() {
  if (!navigator.clipboard) {
    flash("clipboard api unavailable");
    return;
  }

  if (!canUseOutput.value) {
    flash("no valid output to copy");
    return;
  }

  try {
    await navigator.clipboard.writeText(result.value.output);
    flash("output copied to clipboard");
  } catch {
    flash("clipboard copy blocked");
  }
}

function useOutputAsInput() {
  if (!canUseOutput.value) {
    flash("no valid output to reuse");
    return;
  }

  input.value = result.value.output;
  flash("output moved into input");
}

function clearWorkspace() {
  input.value = "";
  flash("workspace cleared");
}

function onKeydown(event: KeyboardEvent) {
  const mod = event.metaKey || event.ctrlKey;
  if (!mod) {
    return;
  }

  const key = event.key.toLowerCase();
  if (key === "enter") {
    event.preventDefault();
    if (event.shiftKey) {
      useOutputAsInput();
    } else {
      void copyOutput();
    }
  }

  if (key === "k") {
    event.preventDefault();
    clearWorkspace();
  }
}

watch(theme, (mode) => {
  applyTheme(mode);
  if (!props.embedded) {
    localStorage.setItem(themeKey, mode);
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

watch(input, (value) => {
  localStorage.setItem(draftKey, value);
});

watch(selectedToolId, (value) => {
  localStorage.setItem(toolKey, value);
});

onMounted(() => {
  const storedDraft = localStorage.getItem(draftKey);
  const storedTheme = localStorage.getItem(themeKey);
  const storedTool = localStorage.getItem(toolKey);

  if (storedDraft) {
    input.value = storedDraft;
  }

  if (storedTool && toolMap.has(storedTool as ToolId)) {
    selectedToolId.value = storedTool as ToolId;
  }

  theme.value = props.embedded
    ? props.sharedTheme
    : storedTheme === "light" || storedTheme === "dark"
      ? storedTheme
      : window.matchMedia("(prefers-color-scheme: light)").matches
        ? "light"
        : "dark";

  applyTheme(theme.value);
  window.addEventListener("keydown", onKeydown);
  inputEl.value?.focus();
});

onBeforeUnmount(() => {
  window.removeEventListener("keydown", onKeydown);
  window.clearTimeout(noticeTimer);
});
</script>

<template>
  <div class="app" :class="{ embedded: props.embedded }">
    <section class="toolbar">
      <div class="filepill current-tool">
        <span class="prompt">›</span>
        <span class="filename">{{ selectedTool.label }}</span>
      </div>

      <div class="toolbar-actions">
        <button class="action icon-action accent-btn" title="paste input" aria-label="paste input" @click="pasteInput">
          <svg viewBox="0 0 20 20" aria-hidden="true">
            <path d="M7 4.5h6M8 3h4a1 1 0 0 1 1 1v1H7V4a1 1 0 0 1 1-1Z" fill="none" stroke="currentColor" stroke-linejoin="round" stroke-width="1.5" />
            <rect x="5.5" y="5.5" width="9" height="10.5" rx="1.5" fill="none" stroke="currentColor" stroke-width="1.5" />
          </svg>
        </button>
        <button class="action icon-action" :disabled="!canUseOutput" title="copy output" aria-label="copy output" @click="copyOutput">
          <svg viewBox="0 0 20 20" aria-hidden="true">
            <rect x="7" y="4.5" width="8.5" height="11" rx="1.5" fill="none" stroke="currentColor" stroke-width="1.5" />
            <path d="M5.5 7V14a1.5 1.5 0 0 0 1.5 1.5" fill="none" stroke="currentColor" stroke-linecap="round" stroke-width="1.5" />
          </svg>
        </button>
        <button class="action icon-action" :disabled="!canUseOutput" title="use output as input" aria-label="use output as input" @click="useOutputAsInput">
          <svg viewBox="0 0 20 20" aria-hidden="true">
            <path d="M5 6.5h8.5a2 2 0 0 1 0 4H8.5" fill="none" stroke="currentColor" stroke-linecap="round" stroke-width="1.5" />
            <path d="m8.5 8.5-2-2 2-2" fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" stroke-width="1.5" />
            <path d="M15 13.5H6.5a2 2 0 0 1 0-4H11.5" fill="none" stroke="currentColor" stroke-linecap="round" stroke-width="1.5" opacity="0.5" />
          </svg>
        </button>
        <button class="action icon-action danger-btn" :disabled="!inputCharCount" title="clear workspace" aria-label="clear workspace" @click="clearWorkspace">
          <svg viewBox="0 0 20 20" aria-hidden="true">
            <path d="M6 6l8 8M14 6l-8 8" fill="none" stroke="currentColor" stroke-linecap="round" stroke-width="1.7" />
          </svg>
        </button>
      </div>
    </section>

    <main class="workspace">
      <aside class="panel sidebar">
        <header class="sidebar-head">
          <div>
            <span class="panel-eyebrow">toolset</span>
            <h2>live operators</h2>
          </div>
        </header>

        <div class="group-tabs" role="tablist" aria-label="tool groups">
          <button
            v-for="group in groups"
            :key="group.id"
            class="group-tab"
            :class="{ active: group.id === selectedGroup.id }"
            :aria-pressed="group.id === selectedGroup.id"
            @click="setGroup(group.id)"
          >
            {{ group.shortName }}
          </button>
        </div>

        <section class="group-block">
          <div class="group-copy compact">
            <h3>{{ selectedGroup.name }}</h3>
          </div>

          <div class="tool-grid">
            <button
              v-for="tool in currentGroupView.tools"
              :key="tool.id"
              class="tool-btn"
              :class="{ active: tool.id === selectedToolId }"
              :title="tool.label"
              @click="setTool(tool.id)"
            >
              <span class="tool-title">{{ tool.label }}</span>
            </button>
          </div>
        </section>
      </aside>

      <section class="editor-grid">
        <article class="panel editor-panel">
          <header class="panel-head">
            <div>
              <span class="panel-eyebrow">input</span>
              <h2>source text</h2>
            </div>
            <span class="panel-meta">{{ inputLineCount }} lines</span>
          </header>

          <textarea
            ref="inputEl"
            v-model="input"
            class="editor"
            spellcheck="false"
            :placeholder="selectedTool.inputHint"
          />
        </article>

        <article class="panel editor-panel output-panel" :class="{ danger: result.error }">
          <header class="panel-head">
            <div>
              <span class="panel-eyebrow">output</span>
              <h2>{{ selectedTool.label }}</h2>
            </div>
            <span class="panel-meta">{{ outputLineCount }} lines</span>
          </header>

          <div v-if="result.error" class="error-banner">
            <strong>transform error</strong>
            <span>{{ result.error }}</span>
          </div>

          <textarea
            class="editor output"
            :value="result.output"
            spellcheck="false"
            readonly
            :placeholder="selectedTool.outputHint"
          />
        </article>
      </section>
    </main>

  </div>
</template>

<style scoped>
.app {
  height: 100%;
  display: flex;
  flex-direction: column;
  gap: 14px;
  padding: 18px;
}

.app.embedded {
  padding-bottom: 0;
}

.topbar,
.toolbar,
.statusbar,
.brand,
.topright,
.toolbar-actions,
.name-row,
.panel-head {
  display: flex;
  align-items: center;
  gap: 12px;
}

.topbar,
.toolbar,
.statusbar,
.panel-head {
  justify-content: space-between;
}

.module-topbar {
  justify-content: flex-end;
}

.brand {
  min-width: 0;
}

.brand-copy {
  min-width: 0;
}

.logo,
.panel,
.stat,
.filepill,
.theme-btn,
.action {
  border: 1px solid var(--border);
  background: var(--panel);
}

.logo {
  flex: 0 0 auto;
  width: 40px;
  height: 40px;
  display: grid;
  place-items: center;
  border-radius: 12px;
  color: var(--accent);
  box-shadow: inset 0 0 0 1px var(--accent-glow);
}

.logo svg {
  width: 22px;
  height: 22px;
}

.name {
  font-size: 18px;
  font-weight: 700;
  letter-spacing: -0.03em;
}

.accent {
  color: var(--accent);
}

.ver {
  color: var(--text-faint);
  font-size: 11px;
  letter-spacing: 0.14em;
  text-transform: uppercase;
}

.tagline {
  margin: 2px 0 0;
  color: var(--text-dim);
  font-size: 12px;
}

.stat,
.filepill {
  padding: 9px 11px;
  border-radius: 10px;
  color: var(--text-dim);
  white-space: nowrap;
}

.count,
.filename {
  color: var(--text);
}

.dot,
.prompt {
  color: var(--accent);
}

.prompt {
  flex: 0 0 auto;
  font-weight: 700;
}

.current-tool {
  display: flex;
  align-items: center;
  gap: 10px;
  min-width: 0;
  flex: 1;
}

.tool-summary-inline {
  display: none;
}

.filename {
  font-weight: 600;
}

.theme-btn,
.action,
.tool-btn {
  transition:
    border-color 120ms ease,
    color 120ms ease,
    box-shadow 120ms ease,
    background-color 120ms ease;
}

.theme-btn,
.action {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border-radius: 10px;
}

.theme-btn {
  width: 36px;
  height: 36px;
  color: var(--text-dim);
}

.action {
  width: 28px;
  height: 28px;
  padding: 0;
  border-radius: 7px;
  color: var(--text-dim);
}

.toolbar-actions {
  gap: 6px;
}

.action svg {
  width: 14px;
  height: 14px;
}

.accent-btn {
  color: var(--accent);
  border-color: var(--accent-dim);
}

.danger-btn {
  color: var(--danger);
}

.theme-btn:hover,
.theme-btn:focus-visible,
.action:hover:not(:disabled),
.action:focus-visible,
.tool-btn:hover,
.tool-btn:focus-visible {
  border-color: var(--accent-dim);
  color: var(--text);
  box-shadow: 0 0 0 4px var(--accent-glow);
  outline: none;
}

.action:disabled {
  opacity: 0.35;
  cursor: not-allowed;
}

.workspace {
  min-height: 0;
  flex: 1;
  display: grid;
  grid-template-columns: 300px minmax(0, 1fr);
  gap: 14px;
}

.panel {
  min-height: 0;
  border-radius: 18px;
  box-shadow: none;
}

.sidebar {
  display: flex;
  flex-direction: column;
  overflow: auto;
  padding: 16px;
  gap: 18px;
}

.sidebar-head {
  display: flex;
  align-items: flex-end;
  justify-content: space-between;
  gap: 12px;
  padding-bottom: 12px;
  border-bottom: 1px solid var(--border-soft);
}

.group-block {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.group-tabs {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: 8px;
}

.group-tab {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  min-width: 0;
  padding: 9px 10px;
  border: 1px solid var(--border-soft);
  border-radius: 11px;
  background: transparent;
  color: var(--text-dim);
  font-size: 11px;
  font-weight: 600;
  text-transform: lowercase;
  transition:
    border-color 120ms ease,
    color 120ms ease,
    box-shadow 120ms ease,
    background-color 120ms ease;
}

.group-tab:hover,
.group-tab:focus-visible {
  border-color: var(--accent-dim);
  color: var(--text);
  box-shadow: 0 0 0 4px var(--accent-glow);
  outline: none;
}

.group-tab.active {
  border-color: var(--accent-dim);
  background: var(--panel);
  color: var(--accent);
  box-shadow: 0 0 0 4px var(--accent-glow);
}

.tool-grid {
  display: grid;
  grid-template-columns: minmax(0, 1fr);
  gap: 8px;
}

.group-copy h3,
.panel-head h2,
.sidebar-head h2 {
  margin: 0;
  font-size: 14px;
}

.group-copy p,
.status,
.hints,
.panel-meta,
.panel-eyebrow {
  color: var(--text-dim);
  font-size: 11px;
}

.group-copy p {
  margin: 4px 0 0;
  line-height: 1.55;
}

.group-copy.compact {
  padding: 2px 2px 6px;
}

.tool-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  min-width: 0;
  height: 46px;
  padding: 7px 10px;
  border: 1px solid transparent;
  border-radius: 12px;
  background: transparent;
  color: var(--text-dim);
  text-align: left;
  transition:
    border-color 120ms ease,
    color 120ms ease,
    box-shadow 120ms ease,
    background-color 120ms ease;
}

.tool-btn:hover,
.tool-btn:focus-visible {
  border-color: var(--accent-dim);
  background: var(--panel);
  color: var(--text);
  box-shadow: 0 0 0 3px var(--accent-glow);
  outline: none;
}

.tool-btn.active {
  border-color: var(--accent-dim);
  background: var(--panel);
  color: var(--accent);
  box-shadow: 0 0 0 4px var(--accent-glow);
}

.tool-title {
  display: -webkit-box;
  width: 100%;
  color: inherit;
  font-size: 12px;
  font-weight: 600;
  line-height: 1.35;
  text-align: left;
  overflow: hidden;
  -webkit-box-orient: vertical;
  -webkit-line-clamp: 2;
  line-clamp: 2;
}

.editor-grid {
  min-height: 0;
  display: grid;
  grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
  gap: 14px;
}

.editor-panel {
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

.panel-head {
  padding: 16px 18px 12px;
  border-bottom: 1px solid var(--border-soft);
}

.output-panel.danger {
  border-color: rgba(255, 93, 108, 0.55);
}

.editor {
  flex: 1;
  width: 100%;
  padding: 18px;
  border: none;
  background: transparent;
  color: var(--text);
  line-height: 1.6;
  outline: none;
}

.editor::placeholder {
  color: var(--text-faint);
}

.output {
  color: var(--success);
}

.error-banner {
  display: flex;
  flex-direction: column;
  gap: 4px;
  margin: 14px 18px 0;
  padding: 11px 12px;
  border: 1px solid rgba(255, 93, 108, 0.45);
  border-radius: 12px;
  background: rgba(255, 93, 108, 0.08);
  color: var(--danger);
  font-size: 11px;
}

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

@media (max-width: 1240px) {
  .workspace {
    grid-template-columns: minmax(0, 1fr);
  }

  .editor-grid {
    grid-template-columns: minmax(0, 1fr);
  }
}

@media (max-width: 920px) {
  .app {
    padding: 14px;
  }

  .topbar,
  .toolbar,
  .statusbar {
    flex-direction: column;
    align-items: stretch;
  }

  .topright,
  .toolbar-actions {
    flex-wrap: wrap;
  }

  .current-tool {
    width: 100%;
  }

  .hints {
    justify-content: flex-start;
  }
}
</style>

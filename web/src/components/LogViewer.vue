<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted, nextTick, watch } from 'vue';

export interface LogLine {
  ts: string;
  stream: string;
  line: string;
}

const props = defineProps<{
  resourceId: string;
}>();

// Ring buffer capacity: SPEC.md §11 (max 5000 lines in memory)
// DOM rendering is optimized with windowing over the filtered items.
const logs = ref<LogLine[]>([]);
const autoScroll = ref(true);
const levelFilter = ref<'ALL' | 'INFO' | 'WARN' | 'ERROR'>('ALL');
const searchQuery = ref('');
const logContainer = ref<HTMLElement | null>(null);
const isConnected = ref(false);
const maxRenderedLines = ref(1000); // Windowing rendering limit for ultra-smooth scrolling
let eventSource: EventSource | null = null;
let lastLogTimestamp: string | null = null;

const filteredLogs = computed(() => {
  return logs.value.filter(item => {
    // Level filter
    if (levelFilter.value !== 'ALL') {
      const upper = item.line.toUpperCase();
      if (!upper.includes(`[${levelFilter.value}]`) && !upper.includes(levelFilter.value)) {
        return false;
      }
    }
    // Search query
    if (searchQuery.value.trim() !== '') {
      const query = searchQuery.value.toLowerCase();
      if (!item.line.toLowerCase().includes(query)) {
        return false;
      }
    }
    return true;
  });
});

// Windowed logs for DOM rendering: render up to maxRenderedLines to prevent DOM bloat
const displayedLogs = computed(() => {
  const filtered = filteredLogs.value;
  if (filtered.length <= maxRenderedLines.value) {
    return filtered;
  }
  // Return the most recent slice
  return filtered.slice(filtered.length - maxRenderedLines.value);
});

async function fetchInitialLogs() {
  try {
    const res = await fetch(`/api/v1/resources/${encodeURIComponent(props.resourceId)}/logs?tail=500`, {
      credentials: 'include',
    });
    if (res.ok) {
      const initial: LogLine[] = await res.json();
      logs.value = initial;
      if (initial.length > 0) {
        lastLogTimestamp = initial[initial.length - 1].ts;
      }
      scrollToBottom();
    }
  } catch (err) {
    console.error('Failed to load initial logs:', err);
  }
}

function connectLogStream() {
  if (eventSource) {
    eventSource.close();
  }

  let url = `/api/v1/resources/${encodeURIComponent(props.resourceId)}/logs/stream`;
  // If reconnecting, pass since timestamp to prevent gap in logs
  if (lastLogTimestamp) {
    url += `?since=${encodeURIComponent(lastLogTimestamp)}`;
  }
  eventSource = new EventSource(url);

  eventSource.onopen = () => {
    isConnected.value = true;
  };

  eventSource.onmessage = (e) => {
    try {
      const logLine: LogLine = JSON.parse(e.data);
      logs.value.push(logLine);
      lastLogTimestamp = logLine.ts;
      // Enforce max 5000 lines client memory limit matching server ring buffer
      if (logs.value.length > 5000) {
        logs.value.shift();
      }
      if (autoScroll.value) {
        scrollToBottom();
      }
    } catch (err) {
      console.error('Failed to parse log line:', err);
    }
  };

  eventSource.onerror = () => {
    isConnected.value = false;
  };
}

function scrollToBottom() {
  nextTick(() => {
    if (logContainer.value) {
      logContainer.value.scrollTop = logContainer.value.scrollHeight;
    }
  });
}

function handleScroll() {
  if (!logContainer.value) return;
  const { scrollTop, scrollHeight, clientHeight } = logContainer.value;
  const isAtBottom = scrollHeight - scrollTop - clientHeight < 40;
  autoScroll.value = isAtBottom;
}

function downloadLogs() {
  const content = filteredLogs.value.map(l => `[${l.ts}] [${l.stream}] ${l.line}`).join('\n');
  const blob = new Blob([content], { type: 'text/plain;charset=utf-8' });
  const url = URL.createObjectURL(blob);
  const a = document.createElement('a');
  a.href = url;
  a.download = `${props.resourceId.replace(/[:/]/g, '_')}_logs.txt`;
  a.click();
  URL.revokeObjectURL(url);
}

function clearLogs() {
  logs.value = [];
}

onMounted(() => {
  fetchInitialLogs();
  connectLogStream();
});

onUnmounted(() => {
  if (eventSource) {
    eventSource.close();
    eventSource = null;
  }
});

watch(() => props.resourceId, () => {
  logs.value = [];
  lastLogTimestamp = null;
  fetchInitialLogs();
  connectLogStream();
});
</script>

<template>
  <div class="log-viewer-card card">
    <div class="log-toolbar">
      <div class="toolbar-left">
        <div class="connection-badge" :class="isConnected ? 'connected' : 'disconnected'">
          <span class="dot"></span>
          {{ isConnected ? 'LIVE STREAM' : 'DISCONNECTED' }}
        </div>
        <span class="log-count">{{ filteredLogs.length }} lines</span>
        <span class="ring-buffer-notice">※ ダウンロードはリングバッファ内 (最大 5000 行) のみ対象</span>
      </div>

      <div class="toolbar-right">
        <!-- Search -->
        <input
          v-model="searchQuery"
          type="text"
          class="input search-input"
          placeholder="ログを検索..."
          id="log-search-input"
        />

        <!-- Level Filter -->
        <select v-model="levelFilter" class="input level-select" id="log-level-select">
          <option value="ALL">ALL LEVELS</option>
          <option value="INFO">INFO</option>
          <option value="WARN">WARN</option>
          <option value="ERROR">ERROR</option>
        </select>

        <!-- Auto Scroll Toggle -->
        <button
          class="btn btn-sm"
          :class="autoScroll ? 'btn-primary' : 'btn-secondary'"
          id="btn-toggle-autoscroll"
          @click="autoScroll = !autoScroll; if (autoScroll) scrollToBottom();"
          title="自動スクロール追従"
        >
          ⬇️ 自動スクロール: {{ autoScroll ? 'ON' : 'OFF' }}
        </button>

        <!-- Download Logs -->
        <button
          class="btn btn-sm btn-secondary"
          id="btn-download-logs"
          @click="downloadLogs"
          title="ログをダウンロード (リングバッファ最大5000行)"
        >
          💾 保存
        </button>

        <!-- Clear View -->
        <button
          class="btn btn-sm btn-secondary"
          id="btn-clear-logs"
          @click="clearLogs"
          title="画面ログをクリア"
        >
          🗑️ クリア
        </button>
      </div>
    </div>

    <!-- Terminal Console Area -->
    <div
      ref="logContainer"
      class="terminal-container"
      id="log-terminal-container"
      @scroll="handleScroll"
    >
      <div v-if="filteredLogs.length === 0" class="empty-logs">
        ログがありません (またはフィルタに一致しません)
      </div>
      <div
        v-for="(item, idx) in displayedLogs"
        :key="idx"
        class="log-row"
        :class="`stream-${item.stream}`"
      >
        <span class="log-ts">{{ item.ts.substring(11, 19) }}</span>
        <span class="log-stream">[{{ item.stream }}]</span>
        <span class="log-text">{{ item.line }}</span>
      </div>
    </div>
  </div>
</template>

<style scoped>
.log-viewer-card {
  display: flex;
  flex-direction: column;
  height: 520px;
  background: var(--color-surface);
  border: 1px solid var(--color-border);
  border-radius: var(--radius-lg);
  overflow: hidden;
  box-shadow: var(--shadow-sm);
}

.log-toolbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: var(--space-3) var(--space-4);
  background: var(--color-surface-hover);
  border-bottom: 1px solid var(--color-border);
  gap: var(--space-3);
  flex-wrap: wrap;
}

.toolbar-left {
  display: flex;
  align-items: center;
  gap: var(--space-3);
}

.toolbar-right {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  flex-wrap: wrap;
}

.connection-badge {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  font-size: 0.75rem;
  font-weight: 700;
  padding: 2px 8px;
  border-radius: var(--radius-full);
  letter-spacing: 0.05em;
}

.connection-badge.connected {
  background: rgba(16, 185, 129, 0.15);
  color: var(--color-success);
}

.connection-badge.disconnected {
  background: rgba(239, 68, 68, 0.15);
  color: var(--color-danger);
}

.dot {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: currentColor;
}

.connection-badge.connected .dot {
  animation: pulse 2s infinite;
}

@keyframes pulse {
  0%, 100% { opacity: 1; }
  50% { opacity: 0.4; }
}

.log-count {
  font-size: 0.8rem;
  color: var(--color-text-muted);
}

.ring-buffer-notice {
  font-size: 0.75rem;
  color: var(--color-warning);
  background: rgba(245, 158, 11, 0.1);
  padding: 2px 6px;
  border-radius: var(--radius-sm);
}

.search-input {
  width: 180px;
  padding: 4px 8px;
  font-size: 0.85rem;
}

.level-select {
  padding: 4px 8px;
  font-size: 0.85rem;
  width: 130px;
}

.terminal-container {
  flex: 1;
  background: #090d16;
  color: #e2e8f0;
  font-family: 'JetBrains Mono', 'Fira Code', ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
  font-size: 0.82rem;
  line-height: 1.5;
  padding: var(--space-3) var(--space-4);
  overflow-y: auto;
  white-space: pre-wrap;
  word-break: break-all;
}

.empty-logs {
  color: #64748b;
  text-align: center;
  padding: var(--space-8);
  font-style: italic;
}

.log-row {
  display: flex;
  gap: var(--space-2);
  margin-bottom: 2px;
}

.log-ts {
  color: #64748b;
  flex-shrink: 0;
  user-select: none;
}

.log-stream {
  color: #38bdf8;
  flex-shrink: 0;
  user-select: none;
}

.stream-stderr .log-stream,
.stream-stderr .log-text {
  color: #f87171;
}

.stream-journal .log-stream {
  color: #a78bfa;
}

.log-text {
  flex: 1;
}
</style>

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

const logs = ref<LogLine[]>([]);
const autoScroll = ref(true);
const levelFilter = ref<'ALL' | 'INFO' | 'WARN' | 'ERROR'>('ALL');
const searchQuery = ref('');
const logContainer = ref<HTMLElement | null>(null);
const isConnected = ref(false);
let eventSource: EventSource | null = null;

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

async function fetchInitialLogs() {
  try {
    const res = await fetch(`/api/v1/resources/${encodeURIComponent(props.resourceId)}/logs?tail=500`, {
      credentials: 'include',
    });
    if (res.ok) {
      logs.value = await res.json();
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

  const url = `/api/v1/resources/${encodeURIComponent(props.resourceId)}/logs/stream`;
  eventSource = new EventSource(url);

  eventSource.onopen = () => {
    isConnected.value = true;
  };

  eventSource.onmessage = (e) => {
    try {
      const logLine: LogLine = JSON.parse(e.data);
      logs.value.push(logLine);
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
        v-for="(item, idx) in filteredLogs"
        :key="idx"
        class="log-row"
        :class="{
          'row-warn': item.line.includes('[WARN]') || item.line.includes('WARN'),
          'row-error': item.line.includes('[ERROR]') || item.line.includes('ERROR')
        }"
      >
        <span class="log-ts">{{ item.ts.substring(11, 19) }}</span>
        <span class="log-stream" :class="item.stream">{{ item.stream }}</span>
        <span class="log-line">{{ item.line }}</span>
      </div>
    </div>
  </div>
</template>

<style scoped>
.log-viewer-card {
  display: flex;
  flex-direction: column;
  height: 600px;
  background: #0f141c;
  border: 1px solid var(--color-border);
  border-radius: var(--radius-lg);
  overflow: hidden;
}

.log-toolbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 0.75rem 1rem;
  background: #161d28;
  border-bottom: 1px solid #232c3d;
  flex-wrap: wrap;
  gap: 0.75rem;
}

.toolbar-left {
  display: flex;
  align-items: center;
  gap: 0.75rem;
  flex-wrap: wrap;
}

.connection-badge {
  display: inline-flex;
  align-items: center;
  gap: 0.4rem;
  font-size: 0.75rem;
  font-weight: 700;
  padding: 0.2rem 0.5rem;
  border-radius: 9999px;
  letter-spacing: 0.05em;
}

.connection-badge.connected {
  background: rgba(16, 185, 129, 0.15);
  color: #10b981;
}

.connection-badge.disconnected {
  background: rgba(239, 68, 68, 0.15);
  color: #ef4444;
}

.dot {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: currentColor;
}

.log-count {
  font-size: 0.8rem;
  color: #94a3b8;
}

.ring-buffer-notice {
  font-size: 0.75rem;
  color: #64748b;
}

.toolbar-right {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  flex-wrap: wrap;
}

.search-input {
  width: 160px;
  height: 32px;
  font-size: 0.8rem;
  background: #0f141c;
  border-color: #2a3649;
  color: #f1f5f9;
}

.level-select {
  height: 32px;
  font-size: 0.8rem;
  background: #0f141c;
  border-color: #2a3649;
  color: #f1f5f9;
}

.btn-sm {
  padding: 0.35rem 0.65rem;
  font-size: 0.8rem;
}

.terminal-container {
  flex: 1;
  padding: 0.75rem 1rem;
  overflow-y: auto;
  font-family: 'JetBrains Mono', 'Fira Code', 'Courier New', Courier, monospace;
  font-size: 0.85rem;
  line-height: 1.5;
  color: #cbd5e1;
  background: #090d14;
}

.empty-logs {
  color: #64748b;
  text-align: center;
  padding: 3rem 0;
  font-style: italic;
}

.log-row {
  display: flex;
  align-items: baseline;
  gap: 0.75rem;
  white-space: pre-wrap;
  word-break: break-all;
}

.log-row:hover {
  background: rgba(255, 255, 255, 0.03);
}

.log-ts {
  color: #64748b;
  font-size: 0.75rem;
  flex-shrink: 0;
  user-select: none;
}

.log-stream {
  font-size: 0.7rem;
  font-weight: 600;
  padding: 0 0.3rem;
  border-radius: 2px;
  flex-shrink: 0;
  text-transform: uppercase;
}

.log-stream.stdout {
  background: rgba(59, 130, 246, 0.15);
  color: #60a5fa;
}

.log-stream.stderr {
  background: rgba(239, 68, 68, 0.15);
  color: #f87171;
}

.log-stream.journal {
  background: rgba(168, 85, 247, 0.15);
  color: #c084fc;
}

.log-line {
  flex: 1;
}

.row-warn .log-line {
  color: #fbbf24;
}

.row-error .log-line {
  color: #f87171;
  font-weight: 600;
}
</style>

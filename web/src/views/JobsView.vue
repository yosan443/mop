<script setup lang="ts">
import { onMounted, onUnmounted, ref } from 'vue';

export interface JobItem {
  id: string;
  kind: string;
  plugin_id?: string | null;
  status: 'queued' | 'running' | 'succeeded' | 'failed' | 'canceled';
  params_json: string;
  created_by: string;
  created_at: string;
  started_at?: string | null;
  finished_at?: string | null;
  error?: string | null;
}

export interface JobEventItem {
  job_id: string;
  seq: number;
  ts: string;
  level: string;
  message: string;
  data_json?: string | null;
}

const jobs = ref<JobItem[]>([]);
const loading = ref(false);
const error = ref<string | null>(null);

const selectedJobId = ref<string | null>(null);
const jobEvents = ref<Record<string, JobEventItem[]>>({});
const loadingEvents = ref<Record<string, boolean>>({});

let eventSource: EventSource | null = null;

async function fetchJobs() {
  loading.value = true;
  error.value = null;
  try {
    const res = await fetch('/api/v1/jobs');
    if (!res.ok) {
      throw new Error(`Failed to fetch jobs (${res.status})`);
    }
    const data: JobItem[] = await res.json();
    jobs.value = data;
  } catch (err: any) {
    error.value = err.message || 'ジョブ一覧の取得に失敗しました';
  } finally {
    loading.value = false;
  }
}

async function loadJobDetails(id: string) {
  if (selectedJobId.value === id) {
    selectedJobId.value = null;
    return;
  }

  selectedJobId.value = id;
  loadingEvents.value[id] = true;
  try {
    const res = await fetch(`/api/v1/jobs/${encodeURIComponent(id)}`);
    if (res.ok) {
      const data = await res.json();
      if (data.events) {
        jobEvents.value[id] = data.events;
      }
      if (data.job) {
        const idx = jobs.value.findIndex((j) => j.id === id);
        if (idx !== -1) {
          jobs.value[idx] = data.job;
        }
      }
    }
  } catch (e) {
    console.error('Failed to load job events', e);
  } finally {
    loadingEvents.value[id] = false;
  }
}

function initSse() {
  try {
    eventSource = new EventSource('/api/v1/jobs/stream');
    eventSource.onmessage = (event) => {
      try {
        const updatedJob: JobItem = JSON.parse(event.data);
        const idx = jobs.value.findIndex((j) => j.id === updatedJob.id);
        if (idx !== -1) {
          jobs.value[idx] = updatedJob;
        } else {
          jobs.value.unshift(updatedJob);
        }

        // If currently viewed, reload events
        if (selectedJobId.value === updatedJob.id) {
          fetch(`/api/v1/jobs/${encodeURIComponent(updatedJob.id)}`)
            .then((r) => r.json())
            .then((d) => {
              if (d.events) {
                jobEvents.value[updatedJob.id] = d.events;
              }
            })
            .catch(() => {});
        }
      } catch (e) {
        console.error('Failed to parse SSE job message', e);
      }
    };
  } catch (e) {
    console.warn('SSE connection failed', e);
  }
}

function getStatusBadgeClass(status: string) {
  switch (status) {
    case 'running':
      return 'badge-primary';
    case 'succeeded':
      return 'badge-success';
    case 'failed':
      return 'badge-danger';
    case 'queued':
      return 'badge-warning';
    case 'canceled':
      return 'badge-muted';
    default:
      return 'badge-neutral';
  }
}

function extractProgress(event: JobEventItem): number | null {
  if (!event.data_json) return null;
  try {
    const data = JSON.parse(event.data_json);
    if (typeof data.percent === 'number') {
      return data.percent;
    }
  } catch {}
  return null;
}

let pollInterval: any = null;

onMounted(async () => {
  await fetchJobs();
  initSse();
  pollInterval = setInterval(fetchJobs, 2000);
});

onUnmounted(() => {
  if (pollInterval) {
    clearInterval(pollInterval);
    pollInterval = null;
  }
  if (eventSource) {
    eventSource.close();
    eventSource = null;
  }
});
</script>

<template>
  <div class="jobs-layout">
    <!-- Header -->
    <header class="app-header">
      <div class="header-left">
        <router-link to="/" class="btn btn-secondary btn-sm" id="btn-back-dashboard">
          ← ダッシュボード
        </router-link>
        <div class="brand-title">
          <span class="brand-icon">📋</span>
          <h2>ジョブ履歴</h2>
        </div>
      </div>

      <div class="header-right">
        <router-link to="/plugins" class="btn btn-secondary btn-sm" id="nav-plugins">
          🧩 プラグイン
        </router-link>
        <button class="btn btn-secondary btn-sm" @click="fetchJobs" :disabled="loading" id="btn-refresh-jobs">
          🔄 更新
        </button>
      </div>
    </header>

    <!-- Main Content -->
    <main class="main-content">
      <div v-if="error" class="banner banner-error">
        {{ error }}
      </div>

      <div v-if="loading && !jobs.length" class="loading-state">
        <div class="spinner"></div>
        <p>ジョブ情報を読み込み中...</p>
      </div>

      <div v-else-if="!jobs.length" class="empty-state" id="empty-jobs">
        <span class="empty-icon">📭</span>
        <p>実行されたジョブはありません</p>
      </div>

      <div v-else class="jobs-list" id="jobs-list">
        <div
          v-for="job in jobs"
          :key="job.id"
          class="job-card"
          :id="`job-card-${job.id}`"
          @click="loadJobDetails(job.id)"
        >
          <div class="job-card-header">
            <div class="job-main-info">
              <span class="job-kind">{{ job.kind }}</span>
              <span v-if="job.plugin_id" class="job-plugin-tag">{{ job.plugin_id }}</span>
              <span class="job-id">#{{ job.id }}</span>
            </div>

            <div class="job-meta-group">
              <span class="badge" :class="getStatusBadgeClass(job.status)" :id="`job-status-${job.id}`" data-test="job-status">
                {{ job.status.toUpperCase() }}
              </span>
              <span class="job-time">{{ new Date(job.created_at).toLocaleTimeString() }}</span>
            </div>
          </div>

          <!-- Expanded Events / Logs -->
          <div v-if="selectedJobId === job.id" class="job-details-panel" id="job-events-panel">
            <div v-if="loadingEvents[job.id]" class="panel-loading">
              イベント履歴を読み込み中...
            </div>
            <div v-else-if="!jobEvents[job.id]?.length" class="panel-empty">
              記録されたイベントはありません
            </div>
            <div v-else class="events-log-list">
              <div
                v-for="evt in jobEvents[job.id]"
                :key="evt.seq"
                class="event-log-item"
                data-test="job-event"
              >
                <span class="event-seq">#{{ evt.seq }}</span>
                <span class="event-level" :class="`level-${evt.level}`">{{ evt.level }}</span>
                <span class="event-msg" data-test="job-event-msg">{{ evt.message }}</span>
                <span
                  v-if="extractProgress(evt) !== null"
                  class="progress-badge"
                  data-test="job-progress"
                >
                  {{ extractProgress(evt) }}%
                </span>
              </div>
            </div>
          </div>
        </div>
      </div>
    </main>
  </div>
</template>

<style scoped>
.jobs-layout {
  min-height: 100vh;
  background-color: var(--bg-app);
  color: var(--text-main);
  display: flex;
  flex-direction: column;
}

.app-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 1rem 2rem;
  background: var(--bg-surface);
  border-bottom: 1px solid var(--border-subtle);
  position: sticky;
  top: 0;
  z-index: 10;
}

.header-left {
  display: flex;
  align-items: center;
  gap: 1.5rem;
}

.brand-title {
  display: flex;
  align-items: center;
  gap: 0.5rem;
}

.brand-title h2 {
  font-size: 1.25rem;
  font-weight: 600;
  margin: 0;
}

.main-content {
  flex: 1;
  max-width: 1000px;
  width: 100%;
  margin: 0 auto;
  padding: 2rem;
  display: flex;
  flex-direction: column;
  gap: 1.5rem;
}

.banner {
  padding: 0.75rem 1rem;
  border-radius: var(--radius-md);
  font-size: 0.9rem;
}

.banner-error {
  background: rgba(239, 68, 68, 0.15);
  border: 1px solid var(--danger);
  color: #fca5a5;
}

.loading-state,
.empty-state {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  padding: 4rem 2rem;
  color: var(--text-muted);
  gap: 1rem;
}

.empty-icon {
  font-size: 3rem;
}

.jobs-list {
  display: flex;
  flex-direction: column;
  gap: 1rem;
}

.job-card {
  background: var(--bg-surface);
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-lg);
  padding: 1.25rem;
  cursor: pointer;
  transition: border-color 0.15s ease;
}

.job-card:hover {
  border-color: var(--border-hover);
}

.job-card-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
}

.job-main-info {
  display: flex;
  align-items: center;
  gap: 0.75rem;
}

.job-kind {
  font-size: 1rem;
  font-weight: 600;
  color: var(--text-main);
}

.job-plugin-tag {
  background: rgba(59, 130, 246, 0.15);
  color: #93c5fd;
  border-radius: var(--radius-sm);
  padding: 2px 8px;
  font-size: 0.8rem;
  font-family: monospace;
}

.job-id {
  font-size: 0.8rem;
  color: var(--text-muted);
  font-family: monospace;
}

.job-meta-group {
  display: flex;
  align-items: center;
  gap: 1rem;
}

.job-time {
  font-size: 0.85rem;
  color: var(--text-muted);
}

.job-details-panel {
  margin-top: 1rem;
  padding-top: 1rem;
  border-top: 1px solid var(--border-subtle);
}

.events-log-list {
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
  font-family: monospace;
  font-size: 0.85rem;
}

.event-log-item {
  display: flex;
  align-items: center;
  gap: 0.75rem;
  padding: 0.4rem 0.6rem;
  background: rgba(0, 0, 0, 0.2);
  border-radius: var(--radius-sm);
}

.event-seq {
  color: var(--text-muted);
  width: 32px;
}

.event-level {
  text-transform: uppercase;
  font-size: 0.75rem;
  font-weight: 600;
  padding: 2px 6px;
  border-radius: var(--radius-sm);
}

.event-level.level-info {
  background: rgba(59, 130, 246, 0.2);
  color: #93c5fd;
}

.event-level.level-error {
  background: rgba(239, 68, 68, 0.2);
  color: #fca5a5;
}

.event-msg {
  flex: 1;
  word-break: break-all;
}

.progress-badge {
  background: rgba(16, 185, 129, 0.2);
  color: #6ee7b7;
  padding: 2px 6px;
  border-radius: var(--radius-sm);
  font-weight: bold;
}
</style>

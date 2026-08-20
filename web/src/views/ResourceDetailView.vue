<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from 'vue';
import { useRoute, useRouter } from 'vue-router';
import { useResourceStore } from '../stores/resources';
import { useAuthStore } from '../stores/auth';
import LogViewer from '../components/LogViewer.vue';
import ConfirmActionModal from '../components/ConfirmActionModal.vue';

const route = useRoute();
const router = useRouter();
const resourceStore = useResourceStore();
const authStore = useAuthStore();

const resourceId = computed(() => route.params.id as string);
const detail = computed(() => resourceStore.details[resourceId.value]);

const showConfirmModal = ref(false);
const pendingAction = ref<'start' | 'stop' | 'restart'>('restart');
const actionLoading = ref(false);
const actionMessage = ref<{ type: 'success' | 'error'; text: string } | null>(null);

const isOperator = computed(() => {
  const role = authStore.user?.role;
  return role === 'admin' || role === 'operator';
});

function statusBadgeClass(status?: string) {
  switch (status) {
    case 'running': return 'badge-success';
    case 'stopped': return 'badge-neutral';
    case 'failed': return 'badge-error';
    case 'degraded': return 'badge-warning';
    case 'restarting': return 'badge-warning';
    default: return 'badge-neutral';
  }
}

function formatBytes(bytes?: number) {
  if (!bytes) return '-';
  const mb = bytes / (1024 * 1024);
  if (mb >= 1024) {
    return `${(mb / 1024).toFixed(2)} GB`;
  }
  return `${mb.toFixed(1)} MB`;
}

function formatUptime(secs?: number) {
  if (!secs) return '-';
  const days = Math.floor(secs / 86400);
  const hours = Math.floor((secs % 86400) / 3600);
  const mins = Math.floor((secs % 3600) / 60);
  if (days > 0) return `${days}d ${hours}h ${mins}m`;
  if (hours > 0) return `${hours}h ${mins}m`;
  return `${mins}m`;
}

function openActionModal(action: 'start' | 'stop' | 'restart') {
  pendingAction.value = action;
  showConfirmModal.value = true;
  actionMessage.value = null;
}

async function handleActionConfirm() {
  showConfirmModal.value = false;
  actionLoading.value = true;
  actionMessage.value = null;
  try {
    const res = await resourceStore.executeAction(resourceId.value, pendingAction.value);
    actionMessage.value = {
      type: 'success',
      text: `操作要求が受理されました (Job ID: ${res.job_id})`,
    };
    // Re-fetch detail after action
    setTimeout(() => {
      resourceStore.fetchResourceDetail(resourceId.value);
    }, 600);
  } catch (err: any) {
    actionMessage.value = {
      type: 'error',
      text: err.message || '操作に失敗しました',
    };
  } finally {
    actionLoading.value = false;
  }
}

onMounted(() => {
  resourceStore.fetchResourceDetail(resourceId.value);
  resourceStore.connectEvents();
});

onUnmounted(() => {
  resourceStore.disconnectEvents();
});
</script>

<template>
  <div class="resource-detail-layout">
    <!-- Header & Breadcrumbs -->
    <div class="detail-header-nav">
      <button class="btn btn-secondary btn-sm" id="btn-back-dashboard" @click="router.push('/')">
        ← ダッシュボードに戻る
      </button>
    </div>

    <!-- Title & Status & Actions Bar -->
    <div class="detail-title-card card">
      <div class="title-left">
        <div class="title-top">
          <span class="kind-tag">{{ detail?.resource.kind }}</span>
          <span v-if="detail?.resource.group_name" class="group-tag">{{ detail.resource.group_name }}</span>
          <span class="badge" :class="statusBadgeClass(detail?.status)" id="detail-status-badge">
            {{ detail?.status || 'UNKNOWN' }}
          </span>
        </div>
        <h1 class="resource-title" id="detail-resource-name">
          {{ detail?.resource.display_name || detail?.resource.name || resourceId }}
        </h1>
        <code class="resource-raw-id">{{ resourceId }}</code>
      </div>

      <!-- Action Buttons (Operator/Admin only) -->
      <div v-if="isOperator" class="action-buttons-group">
        <button
          class="btn btn-secondary"
          id="btn-action-start"
          :disabled="actionLoading || detail?.status === 'running'"
          @click="openActionModal('start')"
        >
          ▶ 起動
        </button>
        <button
          class="btn btn-secondary"
          id="btn-action-stop"
          :disabled="actionLoading || detail?.status === 'stopped'"
          @click="openActionModal('stop')"
        >
          ⏹ 停止
        </button>
        <button
          class="btn btn-primary"
          id="btn-action-restart"
          :disabled="actionLoading"
          @click="openActionModal('restart')"
        >
          🔄 再起動
        </button>
      </div>
    </div>

    <!-- Feedback banner -->
    <div v-if="actionMessage" class="alert" :class="actionMessage.type === 'success' ? 'alert-success' : 'alert-error'">
      {{ actionMessage.text }}
    </div>

    <!-- Metrics & Metadata Grid -->
    <div class="metrics-grid">
      <div class="metric-card card">
        <span class="metric-label">Uptime</span>
        <span class="metric-val" id="metric-uptime">{{ formatUptime(detail?.uptime_secs) }}</span>
      </div>
      <div class="metric-card card">
        <span class="metric-label">Memory</span>
        <span class="metric-val" id="metric-memory">{{ formatBytes(detail?.memory_bytes) }}</span>
      </div>
      <div class="metric-card card">
        <span class="metric-label">CPU Usage</span>
        <span class="metric-val" id="metric-cpu">{{ detail?.cpu_percent ? `${detail.cpu_percent.toFixed(1)}%` : '-' }}</span>
      </div>
      <div class="metric-card card">
        <span class="metric-label">Active State</span>
        <span class="metric-val font-mono" id="metric-active-state">{{ detail?.active_state || '-' }}</span>
      </div>
    </div>

    <!-- Live Log Viewer -->
    <div class="logs-section">
      <h2 class="section-title">ライブログ (Live Logs)</h2>
      <LogViewer :resource-id="resourceId" />
    </div>

    <!-- Action Confirmation Modal -->
    <ConfirmActionModal
      v-if="showConfirmModal"
      :resource-id="resourceId"
      :resource-name="detail?.resource.display_name || detail?.resource.name || resourceId"
      :action="pendingAction"
      @confirm="handleActionConfirm"
      @cancel="showConfirmModal = false"
    />
  </div>
</template>

<style scoped>
.resource-detail-layout {
  display: flex;
  flex-direction: column;
  gap: 1.5rem;
  padding: 1.5rem;
  max-width: 1300px;
  margin: 0 auto;
}

.detail-header-nav {
  display: flex;
  align-items: center;
}

.detail-title-card {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 1.5rem;
  flex-wrap: wrap;
  gap: 1.25rem;
}

.title-left {
  display: flex;
  flex-direction: column;
  gap: 0.4rem;
}

.title-top {
  display: flex;
  align-items: center;
  gap: 0.5rem;
}

.kind-tag, .group-tag {
  font-size: 0.75rem;
  font-weight: 600;
  padding: 0.15rem 0.5rem;
  border-radius: var(--radius-sm);
  background: var(--color-bg-elevated);
  color: var(--color-text-muted);
  border: 1px solid var(--color-border);
}

.resource-title {
  font-size: 1.6rem;
  font-weight: 700;
  color: var(--color-text-primary);
  margin: 0;
}

.resource-raw-id {
  font-size: 0.85rem;
  color: var(--color-text-muted);
}

.action-buttons-group {
  display: flex;
  align-items: center;
  gap: 0.75rem;
}

.metrics-grid {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(200px, 1fr));
  gap: 1rem;
}

.metric-card {
  padding: 1.25rem;
  display: flex;
  flex-direction: column;
  gap: 0.35rem;
}

.metric-label {
  font-size: 0.8rem;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.05em;
  color: var(--color-text-muted);
}

.metric-val {
  font-size: 1.35rem;
  font-weight: 700;
  color: var(--color-text-primary);
}

.font-mono {
  font-family: monospace;
}

.logs-section {
  display: flex;
  flex-direction: column;
  gap: 0.75rem;
}

.section-title {
  font-size: 1.2rem;
  font-weight: 600;
  color: var(--color-text-primary);
  margin: 0;
}
</style>

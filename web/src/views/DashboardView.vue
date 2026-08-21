<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from 'vue';
import { useRouter } from 'vue-router';
import { useAuthStore } from '../stores/auth';
import { useResourceStore, Resource } from '../stores/resources';
import ConfirmActionModal from '../components/ConfirmActionModal.vue';

const authStore = useAuthStore();
const resourceStore = useResourceStore();
const router = useRouter();

const showConfirmModal = ref(false);
const selectedResource = ref<Resource | null>(null);
const selectedAction = ref<'start' | 'stop' | 'restart'>('restart');
const actionLoading = ref(false);
const actionBanner = ref<{ type: 'success' | 'error'; text: string } | null>(null);

const isFakeBackend = computed(() => authStore.meta?.is_fake_backend ?? true);
const isOperator = computed(() => {
  const role = authStore.user?.role;
  return role === 'admin' || role === 'operator';
});

// Group resources by group_name
const groupedResources = computed(() => {
  const groups: Record<string, Resource[]> = {};
  for (const r of resourceStore.resources) {
    const group = r.group_name || 'その他 (Uncategorized)';
    if (!groups[group]) {
      groups[group] = [];
    }
    groups[group].push(r);
  }
  return groups;
});

// Summary metrics
const summary = computed(() => {
  const list = resourceStore.resources;
  let total = list.length;
  let running = 0;
  let stopped = 0;
  let failed = 0;

  for (const r of list) {
    const st = resourceStore.details[r.id]?.status;
    if (st === 'running') running++;
    else if (st === 'stopped') stopped++;
    else if (st === 'failed' || st === 'degraded') failed++;
  }

  return { total, running, stopped, failed };
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
  return `${mb.toFixed(0)} MB`;
}

function formatUptime(secs?: number) {
  if (!secs) return '-';
  const hours = Math.floor(secs / 3600);
  const mins = Math.floor((secs % 3600) / 60);
  if (hours > 0) return `${hours}h ${mins}m`;
  return `${mins}m`;
}

function openAction(resource: Resource, action: 'start' | 'stop' | 'restart') {
  selectedResource.value = resource;
  selectedAction.value = action;
  showConfirmModal.value = true;
  actionBanner.value = null;
}

async function handleActionConfirm() {
  if (!selectedResource.value) return;
  const resId = selectedResource.value.id;
  const action = selectedAction.value;
  showConfirmModal.value = false;
  actionLoading.value = true;
  actionBanner.value = null;

  try {
    const res = await resourceStore.executeAction(resId, action);
    actionBanner.value = {
      type: 'success',
      text: `${selectedResource.value.display_name || selectedResource.value.name} の ${action} ジョブを受け付けました (Job ID: ${res.job_id})`,
    };
    setTimeout(() => {
      resourceStore.fetchResourceDetail(resId);
    }, 600);
  } catch (err: any) {
    actionBanner.value = {
      type: 'error',
      text: err.message || '操作に失敗しました',
    };
  } finally {
    actionLoading.value = false;
  }
}

async function handleLogout() {
  await authStore.logout();
  router.push('/login');
}

onMounted(() => {
  resourceStore.fetchResources();
  resourceStore.connectEvents();
});

onUnmounted(() => {
  resourceStore.disconnectEvents();
});
</script>

<template>
  <div class="dashboard-layout">
    <!-- Header -->
    <header class="app-header">
      <div class="header-brand">
        <span class="brand-logo">⚡</span>
        <span class="brand-name">mop</span>
        <span class="badge badge-success">M2</span>
      </div>

      <div class="header-actions">
        <router-link
          v-if="authStore.user?.role === 'admin'"
          to="/settings/users"
          class="btn btn-secondary btn-sm"
          id="nav-users"
        >
          👥 ユーザー管理
        </router-link>

        <div class="user-pill">
          <span class="user-name" id="current-username">{{ authStore.user?.username }}</span>
          <span class="badge badge-primary" id="current-user-role">{{ authStore.user?.role }}</span>
        </div>

        <button
          class="btn btn-secondary btn-sm"
          id="btn-logout"
          @click="handleLogout"
        >
          ログアウト
        </button>
      </div>
    </header>

    <!-- Main Content Area -->
    <main class="dashboard-content">
      <!-- FAKE BACKEND Warning Banner -->
      <div v-if="isFakeBackend" class="alert alert-fake-backend" id="banner-fake-backend">
        <span class="fake-icon">⚠️</span>
        <div class="fake-text">
          <strong>FAKE BACKEND 有効:</strong> 現在モックバックエンドで動作しています (テスト用 / 本番環境非推奨)。
        </div>
      </div>

      <!-- Action Feedback Banner -->
      <div v-if="actionBanner" class="alert" :class="actionBanner.type === 'success' ? 'alert-success' : 'alert-error'">
        {{ actionBanner.text }}
      </div>

      <!-- Summary Stat Cards -->
      <div class="summary-cards-grid">
        <div class="stat-card card">
          <span class="stat-label">Total Resources</span>
          <span class="stat-val" id="stat-total">{{ summary.total }}</span>
        </div>
        <div class="stat-card card stat-running">
          <span class="stat-label">Running</span>
          <span class="stat-val" id="stat-running">{{ summary.running }}</span>
        </div>
        <div class="stat-card card stat-stopped">
          <span class="stat-label">Stopped</span>
          <span class="stat-val" id="stat-stopped">{{ summary.stopped }}</span>
        </div>
        <div class="stat-card card stat-failed">
          <span class="stat-label">Failed / Degraded</span>
          <span class="stat-val" id="stat-failed">{{ summary.failed }}</span>
        </div>
      </div>

      <!-- Grouped Resource Cards -->
      <div v-for="(groupResources, groupName) in groupedResources" :key="groupName" class="resource-group-section">
        <h2 class="group-title">
          <span class="group-icon">📦</span>
          {{ groupName }}
          <span class="group-count">({{ groupResources.length }})</span>
        </h2>

        <div class="resources-grid">
          <div
            v-for="r in groupResources"
            :key="r.id"
            class="resource-card card"
            :id="`resource-card-${r.id.replace(/[:/.]/g, '-')}`"
          >
            <div class="card-top">
              <div class="card-meta">
                <span class="resource-kind-badge">{{ r.kind }}</span>
                <span class="badge" :class="statusBadgeClass(resourceStore.details[r.id]?.status)">
                  {{ resourceStore.details[r.id]?.status || 'UNKNOWN' }}
                </span>
              </div>
              <router-link
                :to="`/resources/${encodeURIComponent(r.id)}`"
                class="btn btn-sm btn-secondary btn-detail"
                :id="`btn-detail-${r.id.replace(/[:/.]/g, '-')}`"
              >
                詳細・ログ →
              </router-link>
            </div>

            <div class="card-main">
              <h3 class="resource-name" :title="r.name">
                {{ r.display_name || r.name }}
              </h3>
              <code class="resource-raw-name">{{ r.name }}</code>
            </div>

            <!-- Metrics bar -->
            <div class="card-metrics">
              <div class="metric-item">
                <span class="m-label">Uptime</span>
                <span class="m-val">{{ formatUptime(resourceStore.details[r.id]?.uptime_secs) }}</span>
              </div>
              <div class="metric-item">
                <span class="m-label">Mem</span>
                <span class="m-val">{{ formatBytes(resourceStore.details[r.id]?.memory_bytes) }}</span>
              </div>
              <div class="metric-item">
                <span class="m-label">CPU</span>
                <span class="m-val">{{ resourceStore.details[r.id]?.cpu_percent ? `${resourceStore.details[r.id].cpu_percent?.toFixed(1)}%` : '-' }}</span>
              </div>
            </div>

            <!-- Actions Footer (Operator/Admin only) -->
            <div v-if="isOperator" class="card-actions-footer">
              <button
                class="btn btn-xs btn-secondary"
                :id="`btn-start-${r.id.replace(/[:/.]/g, '-')}`"
                :disabled="resourceStore.details[r.id]?.status === 'running'"
                @click="openAction(r, 'start')"
              >
                ▶ 起動
              </button>
              <button
                class="btn btn-xs btn-secondary"
                :id="`btn-stop-${r.id.replace(/[:/.]/g, '-')}`"
                :disabled="resourceStore.details[r.id]?.status === 'stopped'"
                @click="openAction(r, 'stop')"
              >
                ⏹ 停止
              </button>
              <button
                class="btn btn-xs btn-primary"
                :id="`btn-restart-${r.id.replace(/[:/.]/g, '-')}`"
                @click="openAction(r, 'restart')"
              >
                🔄 再起動
              </button>
            </div>
          </div>
        </div>
      </div>
    </main>

    <!-- Action Confirmation Modal -->
    <ConfirmActionModal
      v-if="showConfirmModal && selectedResource"
      :resource-id="selectedResource.id"
      :resource-name="selectedResource.display_name || selectedResource.name"
      :action="selectedAction"
      @confirm="handleActionConfirm"
      @cancel="showConfirmModal = false"
    />
  </div>
</template>

<style scoped>
.dashboard-layout {
  min-height: 100vh;
  display: flex;
  flex-direction: column;
  background: var(--color-bg-base);
}

.app-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 0.85rem 1.5rem;
  background: var(--color-bg-surface);
  border-bottom: 1px solid var(--color-border);
}

.header-brand {
  display: flex;
  align-items: center;
  gap: 0.5rem;
}

.brand-logo {
  font-size: 1.35rem;
}

.brand-name {
  font-size: 1.25rem;
  font-weight: 700;
  letter-spacing: -0.02em;
  color: var(--color-text-primary);
}

.header-actions {
  display: flex;
  align-items: center;
  gap: 0.75rem;
}

.user-pill {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  padding: 0.25rem 0.65rem;
  background: var(--color-bg-elevated);
  border: 1px solid var(--color-border);
  border-radius: 9999px;
  font-size: 0.85rem;
}

.user-name {
  font-weight: 600;
  color: var(--color-text-primary);
}

.dashboard-content {
  flex: 1;
  padding: 1.5rem;
  max-width: 1300px;
  width: 100%;
  margin: 0 auto;
  display: flex;
  flex-direction: column;
  gap: 1.5rem;
}

.alert-fake-backend {
  display: flex;
  align-items: center;
  gap: 0.75rem;
  background: rgba(245, 158, 11, 0.12);
  border: 1px solid rgba(245, 158, 11, 0.35);
  color: #fbbf24;
  padding: 0.85rem 1.25rem;
  border-radius: var(--radius-md);
  font-size: 0.9rem;
}

.fake-icon {
  font-size: 1.25rem;
}

.summary-cards-grid {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(180px, 1fr));
  gap: 1rem;
}

.stat-card {
  padding: 1.15rem;
  display: flex;
  flex-direction: column;
  gap: 0.25rem;
}

.stat-label {
  font-size: 0.75rem;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.05em;
  color: var(--color-text-muted);
}

.stat-val {
  font-size: 1.6rem;
  font-weight: 700;
  color: var(--color-text-primary);
}

.stat-running .stat-val {
  color: #10b981;
}

.stat-stopped .stat-val {
  color: #94a3b8;
}

.stat-failed .stat-val {
  color: #ef4444;
}

.resource-group-section {
  display: flex;
  flex-direction: column;
  gap: 0.85rem;
}

.group-title {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  font-size: 1.15rem;
  font-weight: 600;
  color: var(--color-text-primary);
  margin: 0;
}

.group-count {
  font-size: 0.85rem;
  font-weight: 500;
  color: var(--color-text-muted);
}

.resources-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(320px, 1fr));
  gap: 1.25rem;
}

.resource-card {
  display: flex;
  flex-direction: column;
  padding: 1.25rem;
  gap: 1rem;
  transition: transform 0.15s ease, box-shadow 0.15s ease;
}

.resource-card:hover {
  transform: translateY(-2px);
  box-shadow: var(--shadow-lg);
}

.card-top {
  display: flex;
  align-items: center;
  justify-content: space-between;
}

.card-meta {
  display: flex;
  align-items: center;
  gap: 0.4rem;
}

.resource-kind-badge {
  font-size: 0.7rem;
  font-weight: 600;
  padding: 0.1rem 0.4rem;
  border-radius: var(--radius-sm);
  background: var(--color-bg-elevated);
  border: 1px solid var(--color-border);
  color: var(--color-text-muted);
}

.card-main {
  display: flex;
  flex-direction: column;
  gap: 0.2rem;
}

.resource-name {
  margin: 0;
  font-size: 1.1rem;
  font-weight: 600;
  color: var(--color-text-primary);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.resource-raw-name {
  font-size: 0.75rem;
  color: var(--color-text-muted);
}

.card-metrics {
  display: grid;
  grid-template-columns: repeat(3, 1fr);
  background: var(--color-bg-elevated);
  padding: 0.65rem 0.75rem;
  border-radius: var(--radius-md);
  border: 1px solid var(--color-border);
  font-size: 0.75rem;
}

.metric-item {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 0.15rem;
}

.m-label {
  color: var(--color-text-muted);
  font-size: 0.65rem;
  text-transform: uppercase;
}

.m-val {
  font-weight: 600;
  color: var(--color-text-primary);
}

.card-actions-footer {
  display: flex;
  align-items: center;
  justify-content: flex-end;
  gap: 0.5rem;
  border-top: 1px solid var(--color-border);
  padding-top: 0.75rem;
}

.btn-xs {
  padding: 0.25rem 0.5rem;
  font-size: 0.75rem;
}
</style>

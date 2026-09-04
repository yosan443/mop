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

// Helper to determine if a resource is managed (mop.managed=true)
function isManagedResource(r: Resource): boolean {
  if (r.kind === 'systemd_unit') return true; // Systemd allowlist items are always managed
  if (r.labels_json) {
    try {
      const val = JSON.parse(r.labels_json);
      if (val['mop.managed'] === 'false' || val['is_managed'] === false) return false;
      if (val['mop.managed'] === 'true' || val['is_managed'] === true) return true;
      if (typeof val['managed_count'] === 'number') return val['managed_count'] > 0;
      if (typeof val['managed_containers_count'] === 'number') return val['managed_containers_count'] > 0;
    } catch {}
  }
  return true;
}

// Parse depends_on for a compose service
function getDependsOn(r: Resource): string[] {
  if (!r.labels_json) return [];
  try {
    const val = JSON.parse(r.labels_json);
    if (Array.isArray(val.depends_on)) return val.depends_on;
  } catch {}
  return [];
}

// Get container count info for a compose resource
function getContainerCounts(r: Resource): { total: number; managed: number } | null {
  if (!r.labels_json) return null;
  try {
    const val = JSON.parse(r.labels_json);
    if (val.type === 'compose_project') {
      return {
        total: val.containers_count ?? 0,
        managed: val.managed_containers_count ?? 0,
      };
    }
  } catch {}
  return null;
}

// Compose Projects
const composeProjects = computed(() => {
  return resourceStore.resources.filter(r => r.kind === 'compose_project');
});

// Compose Services grouped by Project
const composeServicesByProject = computed(() => {
  const map: Record<string, Resource[]> = {};
  for (const r of resourceStore.resources) {
    if (r.kind === 'compose_service') {
      const proj = r.group_name || 'default';
      if (!map[proj]) map[proj] = [];
      map[proj].push(r);
    }
  }
  return map;
});

// Standard non-compose resources grouped by group_name
const standardGroupedResources = computed(() => {
  const groups: Record<string, Resource[]> = {};
  for (const r of resourceStore.resources) {
    // Exclude compose projects and services from flat grouping (they are rendered in compose sections)
    // Docker containers that belong to compose are also grouped with their source or standalone
    if (r.kind === 'compose_project' || r.kind === 'compose_service') continue;

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
        <span class="badge badge-success">M4</span>
      </div>

      <div class="header-actions">
        <router-link
          to="/plugins"
          class="btn btn-secondary btn-sm"
          id="nav-plugins"
        >
          🧩 プラグイン
        </router-link>

        <router-link
          to="/jobs"
          class="btn btn-secondary btn-sm"
          id="nav-jobs"
        >
          📋 ジョブ
        </router-link>

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

      <!-- Docker Compose Projects Section (SPEC §9.3 & §21 M3) -->
      <div v-if="composeProjects.length > 0" class="compose-projects-section" id="section-compose-projects">
        <h2 class="section-heading">
          <span class="group-icon">🐙</span>
          Docker Compose Projects
        </h2>

        <div v-for="proj in composeProjects" :key="proj.id" class="compose-project-card card" :id="`resource-card-${proj.id.replace(/[:/.]/g, '-')}`">
          <!-- Project Header -->
          <div class="project-header">
            <div class="project-title-area">
              <div class="project-badges">
                <span class="resource-kind-badge">compose_project</span>
                <span class="badge" :class="statusBadgeClass(resourceStore.details[proj.id]?.status)">
                  {{ resourceStore.details[proj.id]?.status || 'UNKNOWN' }}
                </span>
                <span v-if="getContainerCounts(proj)" class="badge badge-scope">
                  {{ getContainerCounts(proj)?.managed }}/{{ getContainerCounts(proj)?.total }} managed
                </span>
              </div>
              <h3 class="project-name">{{ proj.display_name || proj.name }}</h3>
              <code class="project-raw-id">{{ proj.id }}</code>
            </div>

            <div class="project-actions">
              <router-link
                :to="`/resources/${encodeURIComponent(proj.id)}`"
                class="btn btn-sm btn-secondary btn-detail"
                :id="`btn-detail-${proj.id.replace(/[:/.]/g, '-')}`"
              >
                プロジェクト詳細・ログ →
              </router-link>

              <button
                v-if="isOperator && isManagedResource(proj)"
                class="btn btn-sm btn-primary"
                :id="`btn-restart-${proj.id.replace(/[:/.]/g, '-')}`"
                :disabled="actionLoading"
                @click="openAction(proj, 'restart')"
              >
                🔄 プロジェクト再起動
              </button>
            </div>
          </div>

          <!-- Services Grid inside Project -->
          <div class="project-services-container">
            <h4 class="services-heading">所属サービス (Services)</h4>
            <div class="services-grid">
              <div
                v-for="svc in (composeServicesByProject[proj.name] || [])"
                :key="svc.id"
                class="service-card card"
                :id="`resource-card-${svc.id.replace(/[:/.]/g, '-')}`"
              >
                <div class="card-top">
                  <div class="card-meta">
                    <span class="resource-kind-badge">service</span>
                    <span class="badge" :class="statusBadgeClass(resourceStore.details[svc.id]?.status)">
                      {{ resourceStore.details[svc.id]?.status || 'UNKNOWN' }}
                    </span>
                    <span v-if="!isManagedResource(svc)" class="badge badge-unmanaged" :id="`badge-unmanaged-${svc.id.replace(/[:/.]/g, '-')}`">
                      未管理
                    </span>
                    <span v-else class="badge badge-managed">
                      管理対象
                    </span>
                  </div>
                  <router-link
                    :to="`/resources/${encodeURIComponent(svc.id)}`"
                    class="btn btn-xs btn-secondary"
                    :id="`btn-detail-${svc.id.replace(/[:/.]/g, '-')}`"
                  >
                    詳細 →
                  </router-link>
                </div>

                <div class="card-main">
                  <h4 class="service-name" :title="svc.name">
                    {{ svc.name }}
                  </h4>
                  <div v-if="getDependsOn(svc).length > 0" class="depends-on-row">
                    <span class="depends-label">depends_on:</span>
                    <span v-for="dep in getDependsOn(svc)" :key="dep" class="badge badge-dep">
                      {{ dep }}
                    </span>
                  </div>
                </div>

                <!-- Actions Footer (Operator/Admin & Managed Only) -->
                <div v-if="isOperator" class="card-actions-footer">
                  <template v-if="isManagedResource(svc)">
                    <button
                      class="btn btn-xs btn-secondary"
                      :id="`btn-start-${svc.id.replace(/[:/.]/g, '-')}`"
                      :disabled="resourceStore.details[svc.id]?.status === 'running'"
                      @click="openAction(svc, 'start')"
                    >
                      ▶ 起動
                    </button>
                    <button
                      class="btn btn-xs btn-secondary"
                      :id="`btn-stop-${svc.id.replace(/[:/.]/g, '-')}`"
                      :disabled="resourceStore.details[svc.id]?.status === 'stopped'"
                      @click="openAction(svc, 'stop')"
                    >
                      ⏹ 停止
                    </button>
                    <button
                      class="btn btn-xs btn-primary"
                      :id="`btn-restart-${svc.id.replace(/[:/.]/g, '-')}`"
                      @click="openAction(svc, 'restart')"
                    >
                      🔄 再起動
                    </button>
                  </template>
                  <div v-else class="unmanaged-notice">
                    <span class="unmanaged-hint">※ mop.managed=true なし (保護中)</span>
                  </div>
                </div>
              </div>
            </div>
          </div>
        </div>
      </div>

      <!-- Standard Grouped Resource Cards (Systemd & Standalone Docker) -->
      <div v-for="(groupResources, groupName) in standardGroupedResources" :key="groupName" class="resource-group-section">
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
                <span v-if="!isManagedResource(r)" class="badge badge-unmanaged" :id="`badge-unmanaged-${r.id.replace(/[:/.]/g, '-')}`">
                  未管理
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
              <template v-if="isManagedResource(r)">
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
              </template>
              <div v-else class="unmanaged-notice">
                <span class="unmanaged-hint">※ mop.managed=true なし (保護中)</span>
              </div>
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
      :labels-json="selectedResource.labels_json || resourceStore.details[selectedResource.id]?.resource.labels_json"
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
  background: var(--color-bg);
}

.app-header {
  height: 60px;
  background: var(--color-bg-surface);
  border-bottom: 1px solid var(--color-border);
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 0 1.5rem;
  position: sticky;
  top: 0;
  z-index: 100;
}

.header-brand {
  display: flex;
  align-items: center;
  gap: 0.6rem;
}

.brand-logo {
  font-size: 1.4rem;
}

.brand-name {
  font-weight: 700;
  font-size: 1.25rem;
  color: var(--color-text-primary);
  letter-spacing: -0.5px;
}

.header-actions {
  display: flex;
  align-items: center;
  gap: 1rem;
}

.user-pill {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  padding: 0.3rem 0.6rem;
  background: var(--color-bg-elevated);
  border: 1px solid var(--color-border);
  border-radius: var(--radius-full);
  font-size: 0.85rem;
}

.user-name {
  font-weight: 600;
  color: var(--color-text-primary);
}

.dashboard-content {
  flex: 1;
  padding: 1.5rem;
  max-width: 1400px;
  width: 100%;
  margin: 0 auto;
  display: flex;
  flex-direction: column;
  gap: 1.75rem;
}

.alert-fake-backend {
  background: rgba(245, 158, 11, 0.12);
  border: 1px solid rgba(245, 158, 11, 0.35);
  color: #f59e0b;
  display: flex;
  align-items: center;
  gap: 0.75rem;
  padding: 0.85rem 1.25rem;
  border-radius: var(--radius-md);
  font-size: 0.9rem;
}

.summary-cards-grid {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(220px, 1fr));
  gap: 1rem;
}

.stat-card {
  padding: 1.25rem;
  display: flex;
  flex-direction: column;
  gap: 0.4rem;
}

.stat-label {
  font-size: 0.85rem;
  font-weight: 500;
  color: var(--color-text-muted);
}

.stat-val {
  font-size: 1.8rem;
  font-weight: 700;
  color: var(--color-text-primary);
}

.stat-running .stat-val { color: var(--color-success); }
.stat-stopped .stat-val { color: var(--color-text-muted); }
.stat-failed .stat-val { color: var(--color-error); }

/* Compose Projects Section */
.compose-projects-section {
  display: flex;
  flex-direction: column;
  gap: 1.25rem;
}

.section-heading {
  font-size: 1.2rem;
  font-weight: 700;
  color: var(--color-text-primary);
  display: flex;
  align-items: center;
  gap: 0.5rem;
  margin: 0;
}

.compose-project-card {
  padding: 1.5rem;
  display: flex;
  flex-direction: column;
  gap: 1.25rem;
  border: 1px solid var(--color-border);
  background: var(--color-bg-surface);
}

.project-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  flex-wrap: wrap;
  gap: 1rem;
  padding-bottom: 1rem;
  border-bottom: 1px solid var(--color-border);
}

.project-title-area {
  display: flex;
  flex-direction: column;
  gap: 0.35rem;
}

.project-badges {
  display: flex;
  align-items: center;
  gap: 0.5rem;
}

.project-name {
  font-size: 1.35rem;
  font-weight: 700;
  color: var(--color-text-primary);
  margin: 0;
}

.project-raw-id {
  font-size: 0.8rem;
  color: var(--color-text-muted);
}

.project-actions {
  display: flex;
  align-items: center;
  gap: 0.75rem;
}

.project-services-container {
  display: flex;
  flex-direction: column;
  gap: 0.75rem;
}

.services-heading {
  font-size: 0.95rem;
  font-weight: 600;
  color: var(--color-text-muted);
  margin: 0;
}

.services-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(280px, 1fr));
  gap: 1rem;
}

.service-card {
  padding: 1.1rem;
  display: flex;
  flex-direction: column;
  gap: 0.75rem;
  background: var(--color-bg-elevated);
  border: 1px solid var(--color-border);
}

.service-name {
  font-size: 1.05rem;
  font-weight: 600;
  color: var(--color-text-primary);
  margin: 0;
}

.depends-on-row {
  display: flex;
  align-items: center;
  gap: 0.4rem;
  flex-wrap: wrap;
  margin-top: 0.35rem;
}

.depends-label {
  font-size: 0.75rem;
  color: var(--color-text-muted);
}

.badge-dep {
  font-size: 0.7rem;
  background: rgba(99, 102, 241, 0.15);
  color: #818cf8;
  border: 1px solid rgba(99, 102, 241, 0.3);
  padding: 0.1rem 0.4rem;
  border-radius: 4px;
}

.badge-scope {
  font-size: 0.75rem;
  background: rgba(148, 163, 184, 0.15);
  color: var(--color-text-muted);
  border: 1px solid var(--color-border);
}

.badge-managed {
  font-size: 0.7rem;
  padding: 0.1rem 0.4rem;
  background: rgba(16, 185, 129, 0.12);
  color: #10b981;
  border: 1px solid rgba(16, 185, 129, 0.25);
  border-radius: 4px;
}

.badge-unmanaged {
  font-size: 0.7rem;
  padding: 0.1rem 0.4rem;
  background: rgba(148, 163, 184, 0.15);
  color: #94a3b8;
  border: 1px solid rgba(148, 163, 184, 0.3);
  border-radius: 4px;
}

.unmanaged-notice {
  font-size: 0.75rem;
  color: var(--color-text-muted);
  padding: 0.25rem 0;
}

/* Grouped Standard Resources */
.resource-group-section {
  display: flex;
  flex-direction: column;
  gap: 1rem;
}

.group-title {
  font-size: 1.15rem;
  font-weight: 600;
  color: var(--color-text-primary);
  display: flex;
  align-items: center;
  gap: 0.5rem;
  margin: 0;
}

.group-count {
  font-size: 0.9rem;
  font-weight: 400;
  color: var(--color-text-muted);
}

.resources-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(320px, 1fr));
  gap: 1.25rem;
}

.resource-card {
  padding: 1.25rem;
  display: flex;
  flex-direction: column;
  gap: 1rem;
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
  padding: 0.15rem 0.45rem;
  border-radius: var(--radius-sm);
  background: var(--color-bg-elevated);
  color: var(--color-text-muted);
  border: 1px solid var(--color-border);
  text-transform: uppercase;
}

.card-main {
  display: flex;
  flex-direction: column;
  gap: 0.25rem;
}

.resource-name {
  font-size: 1.15rem;
  font-weight: 600;
  color: var(--color-text-primary);
  margin: 0;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.resource-raw-name {
  font-size: 0.8rem;
  color: var(--color-text-muted);
}

.card-metrics {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 0.6rem 0.75rem;
  background: var(--color-bg-elevated);
  border-radius: var(--radius-sm);
}

.metric-item {
  display: flex;
  flex-direction: column;
  gap: 0.15rem;
}

.m-label {
  font-size: 0.7rem;
  color: var(--color-text-muted);
}

.m-val {
  font-size: 0.85rem;
  font-weight: 600;
  color: var(--color-text-primary);
}

.card-actions-footer {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  padding-top: 0.5rem;
  border-top: 1px solid var(--color-border);
}
</style>

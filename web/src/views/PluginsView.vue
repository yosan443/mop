<script setup lang="ts">
import { computed, onMounted, ref } from 'vue';
import { useAuthStore } from '../stores/auth';
import { usePluginStore, PluginItem } from '../stores/plugins';
import PluginSettingsModal from '../components/PluginSettingsModal.vue';
import PluginCapabilityModal from '../components/PluginCapabilityModal.vue';

const authStore = useAuthStore();
const pluginStore = usePluginStore();

const isAdmin = computed(() => authStore.user?.role === 'admin');

const selectedPluginForSettings = ref<PluginItem | null>(null);
const showSettingsModal = ref(false);

const selectedPluginForEnable = ref<PluginItem | null>(null);
const showCapabilityModal = ref(false);
const enableLoading = ref(false);

const actionBanner = ref<{ type: 'success' | 'error'; text: string } | null>(null);

onMounted(() => {
  pluginStore.fetchPlugins();
});

function hasUi(plugin: PluginItem): boolean {
  if (!plugin.manifest_json) return false;
  try {
    const m = JSON.parse(plugin.manifest_json);
    return !!(m.ui && m.ui.entry && m.ui.element);
  } catch {
    return false;
  }
}

function openSettings(plugin: PluginItem) {
  selectedPluginForSettings.value = plugin;
  showSettingsModal.value = true;
}

function openEnableModal(plugin: PluginItem) {
  selectedPluginForEnable.value = plugin;
  showCapabilityModal.value = true;
}

async function handleConfirmEnable() {
  if (!selectedPluginForEnable.value) return;
  enableLoading.value = true;
  actionBanner.value = null;
  const pluginId = selectedPluginForEnable.value.id;

  try {
    await pluginStore.enablePlugin(pluginId);
    showCapabilityModal.value = false;
    actionBanner.value = {
      type: 'success',
      text: `プラグイン '${pluginId}' を有効化し、プロセスを起動しました`,
    };
  } catch (err: any) {
    actionBanner.value = {
      type: 'error',
      text: err.message || 'プラグインの有効化に失敗しました',
    };
  } finally {
    enableLoading.value = false;
  }
}

async function handleDisable(plugin: PluginItem) {
  actionBanner.value = null;
  try {
    await pluginStore.disablePlugin(plugin.id);
    actionBanner.value = {
      type: 'success',
      text: `プラグイン '${plugin.id}' を無効化し、プロセスを停止しました`,
    };
  } catch (err: any) {
    actionBanner.value = {
      type: 'error',
      text: err.message || 'プラグインの無効化に失敗しました',
    };
  }
}

function getStatusBadgeClass(state: string) {
  switch (state) {
    case 'running':
      return 'badge-success';
    case 'enabled':
      return 'badge-primary';
    case 'degraded':
      return 'badge-warning';
    case 'disabled':
      return 'badge-muted';
    default:
      return 'badge-neutral';
  }
}
</script>

<template>
  <div class="plugins-layout">
    <!-- Header -->
    <header class="app-header">
      <div class="header-left">
        <router-link to="/" class="btn btn-secondary btn-sm" id="btn-back-dashboard">
          ← ダッシュボード
        </router-link>
        <div class="brand-title">
          <span class="brand-icon">🧩</span>
          <h2>プラグイン管理</h2>
        </div>
      </div>

      <div class="header-right">
        <button
          class="btn btn-secondary btn-sm"
          :disabled="pluginStore.loading"
          @click="pluginStore.fetchPlugins"
          id="btn-refresh-plugins"
        >
          🔄 更新
        </button>
      </div>
    </header>

    <!-- Main Content -->
    <main class="main-content">
      <div v-if="actionBanner" class="banner" :class="`banner-${actionBanner.type}`">
        {{ actionBanner.text }}
      </div>

      <div v-if="pluginStore.error" class="banner banner-error">
        {{ pluginStore.error }}
      </div>

      <div v-if="pluginStore.loading && !pluginStore.plugins.length" class="loading-state">
        <div class="spinner"></div>
        <p>プラグイン情報を読み込み中...</p>
      </div>

      <div v-else-if="!pluginStore.plugins.length" class="empty-state">
        <div class="empty-icon">📦</div>
        <h3>インストールされたプラグインはありません</h3>
        <p>プラグインディレクトリ (<code>/var/lib/mop/plugins</code>) に配置してください。</p>
      </div>

      <div v-else class="plugins-grid" id="plugins-list">
        <div
          v-for="plugin in pluginStore.plugins"
          :key="plugin.id"
          class="plugin-card"
          :class="{ 'plugin-enabled': plugin.enabled }"
          :id="`plugin-card-${plugin.id.replace('.', '-')}`"
        >
          <div class="card-header">
            <div class="plugin-title-group">
              <span class="plugin-avatar">🧩</span>
              <div>
                <h3 class="plugin-name">{{ plugin.name }}</h3>
                <span class="plugin-id">{{ plugin.id }}</span>
              </div>
            </div>
            <div class="badge-group">
              <span class="badge" :class="getStatusBadgeClass(plugin.state)">
                {{ plugin.state.toUpperCase() }}
              </span>
              <span class="version-tag">v{{ plugin.version }}</span>
            </div>
          </div>

          <!-- Permissions summary -->
          <div class="card-body">
            <div v-if="plugin.permissions?.length" class="perms-summary">
              <span class="perms-label">付与権限:</span>
              <div class="perms-tags">
                <span
                  v-for="perm in plugin.permissions"
                  :key="perm.id"
                  class="perm-tag"
                >
                  {{ perm.capability }}: {{ perm.value_json }}
                </span>
              </div>
            </div>
          </div>

          <!-- Actions -->
          <div class="card-footer">
            <div class="footer-left">
              <router-link
                v-if="hasUi(plugin) && (plugin.state === 'running' || plugin.state === 'enabled')"
                :to="`/plugins/${plugin.id}`"
                class="btn btn-primary btn-sm"
                :id="`btn-open-ui-${plugin.id.replace('.', '-')}`"
              >
                🖥️ UI を開く
              </router-link>
            </div>

            <div class="footer-right">
              <button
                v-if="isAdmin"
                class="btn btn-secondary btn-sm"
                @click="openSettings(plugin)"
                :id="`btn-settings-${plugin.id.replace('.', '-')}`"
              >
                ⚙️ 設定
              </button>

              <button
                v-if="isAdmin && !plugin.enabled"
                class="btn btn-success btn-sm"
                @click="openEnableModal(plugin)"
                :id="`btn-enable-${plugin.id.replace('.', '-')}`"
              >
                ▶ 有効化
              </button>

              <button
                v-if="isAdmin && plugin.enabled"
                class="btn btn-danger btn-sm"
                @click="handleDisable(plugin)"
                :id="`btn-disable-${plugin.id.replace('.', '-')}`"
              >
                ⏹ 無効化
              </button>
            </div>
          </div>
        </div>
      </div>
    </main>

    <!-- Modals -->
    <PluginSettingsModal
      v-if="selectedPluginForSettings"
      :plugin="selectedPluginForSettings"
      :is-open="showSettingsModal"
      @close="showSettingsModal = false"
      @applied="pluginStore.fetchPlugins"
    />

    <PluginCapabilityModal
      v-if="selectedPluginForEnable"
      :plugin="selectedPluginForEnable"
      :is-open="showCapabilityModal"
      :loading="enableLoading"
      @close="showCapabilityModal = false"
      @confirm="handleConfirmEnable"
    />
  </div>
</template>

<style scoped>
.plugins-layout {
  min-height: 100vh;
  background-color: var(--bg-app);
  display: flex;
  flex-direction: column;
}

.app-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 1rem 2rem;
  background-color: var(--bg-card);
  border-bottom: 1px solid var(--border-subtle);
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
  padding: 2rem;
  max-width: 1200px;
  margin: 0 auto;
  width: 100%;
  flex: 1;
}

.banner {
  padding: 0.75rem 1.25rem;
  border-radius: var(--radius-md);
  margin-bottom: 1.5rem;
  font-size: 0.875rem;
}

.banner-success {
  background-color: rgba(34, 197, 94, 0.15);
  border: 1px solid var(--success);
  color: var(--success);
}

.banner-error {
  background-color: rgba(239, 68, 68, 0.15);
  border: 1px solid var(--danger);
  color: var(--danger);
}

.loading-state,
.empty-state {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  padding: 4rem 2rem;
  text-align: center;
  color: var(--text-muted);
}

.empty-icon {
  font-size: 3rem;
  margin-bottom: 1rem;
}

.spinner {
  width: 36px;
  height: 36px;
  border: 3px solid var(--border-subtle);
  border-top-color: var(--primary);
  border-radius: 50%;
  animation: spin 0.8s linear infinite;
  margin-bottom: 1rem;
}

.plugins-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(360px, 1fr));
  gap: 1.5rem;
}

.plugin-card {
  background-color: var(--bg-card);
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-lg);
  display: flex;
  flex-direction: column;
  transition: transform 0.15s ease, border-color 0.15s ease;
}

.plugin-card:hover {
  border-color: rgba(255, 255, 255, 0.2);
}

.card-header {
  padding: 1.25rem;
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 1rem;
  border-bottom: 1px solid var(--border-subtle);
}

.plugin-title-group {
  display: flex;
  align-items: center;
  gap: 0.75rem;
}

.plugin-avatar {
  font-size: 1.75rem;
}

.plugin-name {
  font-size: 1rem;
  font-weight: 600;
  margin: 0;
}

.plugin-id {
  font-size: 0.75rem;
  font-family: monospace;
  color: var(--text-muted);
}

.badge-group {
  display: flex;
  flex-direction: column;
  align-items: flex-end;
  gap: 0.35rem;
}

.version-tag {
  font-size: 0.75rem;
  color: var(--text-muted);
}

.card-body {
  padding: 1rem 1.25rem;
  flex: 1;
}

.perms-summary {
  display: flex;
  flex-direction: column;
  gap: 0.4rem;
}

.perms-label {
  font-size: 0.75rem;
  font-weight: 600;
  color: var(--text-muted);
}

.perms-tags {
  display: flex;
  flex-wrap: wrap;
  gap: 0.35rem;
}

.perm-tag {
  background-color: var(--bg-surface);
  border: 1px solid var(--border-subtle);
  font-size: 0.7rem;
  font-family: monospace;
  padding: 0.15rem 0.4rem;
  border-radius: 4px;
  color: var(--text-muted);
}

.card-footer {
  padding: 0.875rem 1.25rem;
  display: flex;
  align-items: center;
  justify-content: space-between;
  border-top: 1px solid var(--border-subtle);
  background-color: rgba(0, 0, 0, 0.05);
}

.footer-right {
  display: flex;
  align-items: center;
  gap: 0.5rem;
}

.btn-success {
  background-color: #16a34a;
  color: white;
  border: none;
}

.btn-success:hover {
  background-color: #15803d;
}

.btn-danger {
  background-color: #dc2626;
  color: white;
  border: none;
}

.btn-danger:hover {
  background-color: #b91c1c;
}

@keyframes spin {
  to {
    transform: rotate(360deg);
  }
}
</style>

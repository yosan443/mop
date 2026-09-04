<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue';
import { useRoute } from 'vue-router';
import { useAuthStore } from '../stores/auth';
import { usePluginStore, PluginItem } from '../stores/plugins';

const route = useRoute();
const authStore = useAuthStore();
const pluginStore = usePluginStore();

const pluginId = computed(() => route.params.id as string);
const containerRef = ref<HTMLDivElement | null>(null);

const plugin = ref<PluginItem | null>(null);
const loading = ref(true);
const errorMessage = ref<string | null>(null);

const currentTheme = ref<'dark' | 'light'>('dark');

async function loadAndMountPlugin() {
  loading.value = true;
  errorMessage.value = null;

  if (!pluginStore.plugins.length) {
    await pluginStore.fetchPlugins();
  }

  const p = pluginStore.plugins.find((item) => item.id === pluginId.value);
  if (!p) {
    errorMessage.value = `プラグイン '${pluginId.value}' が見つかりません`;
    loading.value = false;
    return;
  }

  if (!p.enabled) {
    errorMessage.value = `プラグイン '${pluginId.value}' は無効化されています。プラグイン管理画面で有効化してください。`;
    loading.value = false;
    return;
  }

  plugin.value = p;

  // Parse UI entry & element from manifest
  let uiEntry = 'ui/index.js';
  let uiElement = `mop-plugin-${p.id.replace('mop.', '')}`;

  if (p.manifest_json) {
    try {
      const m = JSON.parse(p.manifest_json);
      if (m.ui) {
        if (m.ui.entry) uiEntry = m.ui.entry;
        if (m.ui.element) uiElement = m.ui.element;
      }
    } catch {}
  }

  const entryUrl = `/api/v1/plugins/${encodeURIComponent(p.id)}/ui/${uiEntry}`;

  try {
    // Dynamic import of Custom Element script
    await import(/* @vite-ignore */ entryUrl);

    // Wait until custom element is defined or check immediately
    await customElements.whenDefined(uiElement);

    if (containerRef.value) {
      containerRef.value.innerHTML = '';
      const customEl: any = document.createElement(uiElement);

      const context = {
        pluginId: p.id,
        apiBaseUrl: `/api/v1/plugins/${encodeURIComponent(p.id)}/rpc`,
        currentUser: authStore.user
          ? {
              id: authStore.user.id,
              username: authStore.user.username,
              role: authStore.user.role,
            }
          : null,
        theme: currentTheme.value,
        callRpc: (method: string, params?: any) =>
          pluginStore.callRpc(p.id, method, params),
        showNotification: (type: string, message: string) => {
          console.log(`[Plugin ${p.id}] [${type}] ${message}`);
        },
      };

      customEl.context = context;
      containerRef.value.appendChild(customEl);
    }
  } catch (err: any) {
    errorMessage.value = `プラグイン UI の読み込みに失敗しました: ${err.message || String(err)}`;
  } finally {
    loading.value = false;
  }
}

function toggleTheme() {
  currentTheme.value = currentTheme.value === 'dark' ? 'light' : 'dark';
  window.dispatchEvent(
    new CustomEvent('mop:theme', { detail: { theme: currentTheme.value } })
  );
}

onMounted(() => {
  loadAndMountPlugin();
});

watch(
  () => route.params.id,
  () => {
    loadAndMountPlugin();
  }
);
</script>

<template>
  <div class="plugin-view-layout">
    <!-- Header -->
    <header class="app-header">
      <div class="header-left">
        <router-link to="/plugins" class="btn btn-secondary btn-sm" id="btn-back-plugins">
          ← プラグイン一覧
        </router-link>
        <div v-if="plugin" class="brand-title">
          <span class="brand-icon">🧩</span>
          <h2>{{ plugin.name }}</h2>
          <span class="version-tag">v{{ plugin.version }}</span>
        </div>
      </div>

      <div class="header-right">
        <router-link to="/jobs" class="btn btn-secondary btn-sm" id="nav-jobs">
          📋 ジョブ
        </router-link>
        <button class="btn btn-secondary btn-sm" @click="toggleTheme" id="btn-toggle-theme">
          🌓 テーマ切替 ({{ currentTheme }})
        </button>
        <button
          class="btn btn-secondary btn-sm"
          :disabled="loading"
          @click="loadAndMountPlugin"
          id="btn-reload-plugin-ui"
        >
          🔄 リロード
        </button>
      </div>
    </header>

    <!-- Content / Container -->
    <main class="plugin-main-area">
      <div v-if="errorMessage" class="banner banner-error">
        {{ errorMessage }}
      </div>

      <div v-if="loading" class="loading-state">
        <div class="spinner"></div>
        <p>プラグイン UI をロード中...</p>
      </div>

      <div ref="containerRef" id="plugin-custom-element-mount" class="plugin-mount-point"></div>
    </main>
  </div>
</template>

<style scoped>
.plugin-view-layout {
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

.header-left,
.header-right {
  display: flex;
  align-items: center;
  gap: 1rem;
}

.brand-title {
  display: flex;
  align-items: center;
  gap: 0.5rem;
}

.brand-title h2 {
  font-size: 1.125rem;
  font-weight: 600;
  margin: 0;
}

.version-tag {
  font-size: 0.75rem;
  color: var(--text-muted);
}

.plugin-main-area {
  padding: 2rem;
  max-width: 1200px;
  margin: 0 auto;
  width: 100%;
  flex: 1;
}

.banner-error {
  background-color: rgba(239, 68, 68, 0.15);
  border: 1px solid var(--danger);
  color: var(--danger);
  padding: 1rem;
  border-radius: var(--radius-md);
  margin-bottom: 1.5rem;
}

.loading-state {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  padding: 4rem;
  color: var(--text-muted);
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

.plugin-mount-point {
  width: 100%;
}

@keyframes spin {
  to {
    transform: rotate(360deg);
  }
}
</style>

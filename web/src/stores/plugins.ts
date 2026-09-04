import { defineStore } from 'pinia';
import { ref, computed } from 'vue';

export interface PluginPermission {
  id: string;
  plugin_id: string;
  capability: string;
  value_json: string;
  granted_by: string;
  granted_at: string;
}

export interface PluginItem {
  id: string;
  name: string;
  version: string;
  api_version: string;
  enabled: boolean;
  state: 'installed' | 'enabled' | 'running' | 'degraded' | 'disabled';
  installed_at: string;
  updated_at: string;
  manifest_json?: string;
  permissions?: PluginPermission[];
  applied_settings?: Record<string, any>;
}

export interface SettingsDiffItem {
  key: string;
  old_value?: any;
  new_value?: any;
  change_type: 'added' | 'modified' | 'deleted';
}

export interface SettingsDiff {
  plugin_id: string;
  items: SettingsDiffItem[];
}

export const usePluginStore = defineStore('plugins', () => {
  const plugins = ref<PluginItem[]>([]);
  const loading = ref(false);
  const error = ref<string | null>(null);

  const uiPlugins = computed(() => {
    return plugins.value.filter((p) => {
      if (!p.enabled) return false;
      if (p.manifest_json) {
        try {
          const m = JSON.parse(p.manifest_json);
          return !!(m.ui && m.ui.entry && m.ui.element);
        } catch {}
      }
      return false;
    });
  });

  async function fetchPlugins() {
    loading.value = true;
    error.value = null;
    try {
      const res = await fetch('/api/v1/plugins');
      if (!res.ok) {
        throw new Error(`Failed to fetch plugins (${res.status})`);
      }
      const data = await res.json();
      plugins.value = data;
    } catch (err: any) {
      error.value = err.message || 'プラグイン一覧の取得に失敗しました';
    } finally {
      loading.value = false;
    }
  }

  async function enablePlugin(id: string) {
    const res = await fetch(`/api/v1/plugins/${encodeURIComponent(id)}/enable`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
    });
    if (!res.ok) {
      const errData = await res.json().catch(() => ({}));
      throw new Error(errData.error?.message || `有効化に失敗しました (${res.status})`);
    }
    await fetchPlugins();
    return await res.json();
  }

  async function disablePlugin(id: string) {
    const res = await fetch(`/api/v1/plugins/${encodeURIComponent(id)}/disable`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
    });
    if (!res.ok) {
      const errData = await res.json().catch(() => ({}));
      throw new Error(errData.error?.message || `無効化に失敗しました (${res.status})`);
    }
    await fetchPlugins();
    return await res.json();
  }

  async function getSettings(id: string) {
    const res = await fetch(`/api/v1/plugins/${encodeURIComponent(id)}/settings`);
    if (!res.ok) {
      const errData = await res.json().catch(() => ({}));
      throw new Error(errData.error?.message || `設定の取得に失敗しました (${res.status})`);
    }
    return await res.json();
  }

  async function saveSettings(id: string, settings: Record<string, any>) {
    const res = await fetch(`/api/v1/plugins/${encodeURIComponent(id)}/settings`, {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ settings }),
    });
    if (!res.ok) {
      const errData = await res.json().catch(() => ({}));
      throw new Error(errData.error?.message || `設定の保存に失敗しました (${res.status})`);
    }
    return await res.json() as SettingsDiff;
  }

  async function getSettingsDiff(id: string) {
    const res = await fetch(`/api/v1/plugins/${encodeURIComponent(id)}/settings/diff`);
    if (!res.ok) {
      const errData = await res.json().catch(() => ({}));
      throw new Error(errData.error?.message || `差分の取得に失敗しました (${res.status})`);
    }
    return await res.json() as SettingsDiff;
  }

  async function applySettings(id: string) {
    const res = await fetch(`/api/v1/plugins/${encodeURIComponent(id)}/settings/apply`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
    });
    if (!res.ok) {
      const errData = await res.json().catch(() => ({}));
      throw new Error(errData.error?.message || `設定の適用に失敗しました (${res.status})`);
    }
    await fetchPlugins();
    return await res.json();
  }

  async function callRpc(id: string, method: string, params?: any) {
    const res = await fetch(`/api/v1/plugins/${encodeURIComponent(id)}/rpc`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        jsonrpc: '2.0',
        method,
        params: params ?? null,
        id: Date.now(),
      }),
    });
    if (!res.ok) {
      const errData = await res.json().catch(() => ({}));
      throw new Error(errData.error?.message || `RPC呼び出しエラー (${res.status})`);
    }
    const rpcRes = await res.json();
    if (rpcRes.error) {
      throw new Error(rpcRes.error.message || JSON.stringify(rpcRes.error));
    }
    return rpcRes.result;
  }

  return {
    plugins,
    loading,
    error,
    uiPlugins,
    fetchPlugins,
    enablePlugin,
    disablePlugin,
    getSettings,
    saveSettings,
    getSettingsDiff,
    applySettings,
    callRpc,
  };
});

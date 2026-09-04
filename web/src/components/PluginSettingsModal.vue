<script setup lang="ts">
import { ref, watch } from 'vue';
import { usePluginStore, PluginItem, SettingsDiff } from '../stores/plugins';

const props = defineProps<{
  plugin: PluginItem;
  isOpen: boolean;
}>();

const emit = defineEmits<{
  (e: 'close'): void;
  (e: 'applied'): void;
}>();

const pluginStore = usePluginStore();

const activeTab = ref<'edit' | 'diff'>('edit');
const settingsJson = ref('');
const diffData = ref<SettingsDiff | null>(null);
const loading = ref(false);
const errorMessage = ref<string | null>(null);
const successMessage = ref<string | null>(null);

async function loadSettings() {
  if (!props.plugin) return;
  loading.value = true;
  errorMessage.value = null;
  successMessage.value = null;
  try {
    const res = await pluginStore.getSettings(props.plugin.id);
    settingsJson.value = JSON.stringify(res.applied || {}, null, 2);
    diffData.value = res.diff;
  } catch (err: any) {
    errorMessage.value = err.message || '設定の読み込みに失敗しました';
  } finally {
    loading.value = false;
  }
}

watch(
  () => props.isOpen,
  (val) => {
    if (val) {
      activeTab.value = 'edit';
      loadSettings();
    }
  },
  { immediate: true }
);

async function handleSaveDraft() {
  errorMessage.value = null;
  successMessage.value = null;
  loading.value = true;

  try {
    let parsed: Record<string, any>;
    try {
      parsed = JSON.parse(settingsJson.value);
    } catch {
      throw new Error('設定の JSON フォーマットが正しくありません');
    }

    const diff = await pluginStore.saveSettings(props.plugin.id, parsed);
    diffData.value = diff;
    successMessage.value = '下書き設定を保存しました。Diff タブで差分を確認できます。';
  } catch (err: any) {
    errorMessage.value = err.message || '下書きの保存に失敗しました';
  } finally {
    loading.value = false;
  }
}

async function handleApply() {
  errorMessage.value = null;
  successMessage.value = null;
  loading.value = true;

  try {
    await pluginStore.applySettings(props.plugin.id);
    successMessage.value = '設定を適用しました';
    emit('applied');
    setTimeout(() => {
      emit('close');
    }, 800);
  } catch (err: any) {
    errorMessage.value = err.message || '設定の適用に失敗しました';
  } finally {
    loading.value = false;
  }
}
</script>

<template>
  <div v-if="isOpen" class="modal-overlay" @click.self="emit('close')">
    <div class="modal-card">
      <div class="modal-header">
        <div class="title-group">
          <span class="modal-icon">⚙️</span>
          <div>
            <h3 class="modal-title">{{ plugin.name }} の設定</h3>
            <p class="modal-subtitle">ID: {{ plugin.id }} (v{{ plugin.version }})</p>
          </div>
        </div>
        <button class="btn-close" @click="emit('close')">✕</button>
      </div>

      <!-- Tabs -->
      <div class="tab-nav">
        <button
          class="tab-btn"
          :class="{ active: activeTab === 'edit' }"
          @click="activeTab = 'edit'"
          id="tab-settings-edit"
        >
          ✏️ 設定編集
        </button>
        <button
          class="tab-btn"
          :class="{ active: activeTab === 'diff' }"
          @click="activeTab = 'diff'"
          id="tab-settings-diff"
        >
          🔍 差分プレビュー
          <span v-if="diffData?.items?.length" class="diff-count-badge">
            {{ diffData.items.length }}
          </span>
        </button>
      </div>

      <!-- Messages -->
      <div v-if="errorMessage" class="banner banner-error">
        {{ errorMessage }}
      </div>
      <div v-if="successMessage" class="banner banner-success">
        {{ successMessage }}
      </div>

      <div class="modal-body">
        <!-- Edit Tab -->
        <div v-if="activeTab === 'edit'" class="tab-content">
          <label class="form-label">設定 JSON (キー / 値)</label>
          <textarea
            v-model="settingsJson"
            class="code-textarea"
            rows="10"
            placeholder='{ "key": "value" }'
            id="settings-json-input"
          ></textarea>
        </div>

        <!-- Diff Tab -->
        <div v-else class="tab-content">
          <div v-if="!diffData?.items?.length" class="empty-diff">
            変更された未適用の設定はありません。
          </div>
          <div v-else class="diff-list" id="settings-diff-list">
            <div
              v-for="item in diffData.items"
              :key="item.key"
              class="diff-item"
              :class="item.change_type"
            >
              <div class="diff-header">
                <span class="diff-type-badge" :class="item.change_type">
                  {{ item.change_type.toUpperCase() }}
                </span>
                <span class="diff-key">{{ item.key }}</span>
              </div>
              <div class="diff-values">
                <div v-if="item.old_value !== undefined" class="diff-old">
                  - {{ JSON.stringify(item.old_value) }}
                </div>
                <div v-if="item.new_value !== undefined" class="diff-new">
                  + {{ JSON.stringify(item.new_value) }}
                </div>
              </div>
            </div>
          </div>
        </div>
      </div>

      <div class="modal-footer">
        <button class="btn btn-secondary" @click="emit('close')">キャンセル</button>
        <button
          v-if="activeTab === 'edit'"
          class="btn btn-secondary"
          :disabled="loading"
          @click="handleSaveDraft"
          id="btn-save-draft"
        >
          💾 下書き保存
        </button>
        <button
          class="btn btn-primary"
          :disabled="loading"
          @click="handleApply"
          id="btn-apply-settings"
        >
          🚀 設定を適用 (Apply)
        </button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.modal-overlay {
  position: fixed;
  inset: 0;
  background-color: rgba(0, 0, 0, 0.7);
  backdrop-filter: blur(4px);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 1000;
  padding: 1rem;
}

.modal-card {
  background-color: var(--bg-card);
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-lg);
  width: 100%;
  max-width: 600px;
  display: flex;
  flex-direction: column;
  box-shadow: var(--shadow-modal);
  max-height: 90vh;
}

.modal-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 1.25rem 1.5rem;
  border-bottom: 1px solid var(--border-subtle);
}

.title-group {
  display: flex;
  align-items: center;
  gap: 0.75rem;
}

.modal-icon {
  font-size: 1.5rem;
}

.modal-title {
  font-size: 1.125rem;
  font-weight: 600;
  margin: 0;
}

.modal-subtitle {
  font-size: 0.75rem;
  color: var(--text-muted);
  margin: 0;
}

.btn-close {
  background: none;
  border: none;
  font-size: 1.25rem;
  color: var(--text-muted);
  cursor: pointer;
}

.tab-nav {
  display: flex;
  border-bottom: 1px solid var(--border-subtle);
  padding: 0 1.5rem;
  background-color: rgba(0, 0, 0, 0.1);
}

.tab-btn {
  background: none;
  border: none;
  border-bottom: 2px solid transparent;
  padding: 0.75rem 1rem;
  font-size: 0.875rem;
  font-weight: 500;
  color: var(--text-muted);
  cursor: pointer;
  display: flex;
  align-items: center;
  gap: 0.5rem;
}

.tab-btn.active {
  color: var(--primary);
  border-bottom-color: var(--primary);
}

.diff-count-badge {
  background-color: var(--primary);
  color: white;
  border-radius: 9999px;
  padding: 0.1rem 0.4rem;
  font-size: 0.75rem;
}

.modal-body {
  padding: 1.5rem;
  overflow-y: auto;
  flex: 1;
}

.form-label {
  display: block;
  font-size: 0.875rem;
  font-weight: 500;
  margin-bottom: 0.5rem;
}

.code-textarea {
  width: 100%;
  background-color: var(--bg-surface);
  color: var(--text-base);
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-md);
  padding: 0.75rem;
  font-family: monospace;
  font-size: 0.875rem;
  resize: vertical;
}

.empty-diff {
  color: var(--text-muted);
  font-size: 0.875rem;
  text-align: center;
  padding: 2rem;
}

.diff-list {
  display: flex;
  flex-direction: column;
  gap: 0.75rem;
}

.diff-item {
  background-color: var(--bg-surface);
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-md);
  padding: 0.75rem 1rem;
}

.diff-header {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  margin-bottom: 0.5rem;
}

.diff-type-badge {
  font-size: 0.7rem;
  font-weight: 700;
  padding: 0.15rem 0.4rem;
  border-radius: 4px;
}

.diff-type-badge.added {
  background-color: rgba(34, 197, 94, 0.2);
  color: var(--success);
}

.diff-type-badge.modified {
  background-color: rgba(234, 179, 8, 0.2);
  color: var(--warning);
}

.diff-type-badge.deleted {
  background-color: rgba(239, 68, 68, 0.2);
  color: var(--danger);
}

.diff-key {
  font-family: monospace;
  font-weight: 600;
  font-size: 0.875rem;
}

.diff-values {
  font-family: monospace;
  font-size: 0.8rem;
}

.diff-old {
  color: var(--danger);
}

.diff-new {
  color: var(--success);
}

.banner {
  margin: 0.75rem 1.5rem 0;
  padding: 0.75rem 1rem;
  border-radius: var(--radius-md);
  font-size: 0.875rem;
}

.banner-error {
  background-color: rgba(239, 68, 68, 0.15);
  border: 1px solid var(--danger);
  color: var(--danger);
}

.banner-success {
  background-color: rgba(34, 197, 94, 0.15);
  border: 1px solid var(--success);
  color: var(--success);
}

.modal-footer {
  display: flex;
  justify-content: flex-end;
  gap: 0.75rem;
  padding: 1rem 1.5rem;
  border-top: 1px solid var(--border-subtle);
  background-color: rgba(0, 0, 0, 0.1);
}
</style>

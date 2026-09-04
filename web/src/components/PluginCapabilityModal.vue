<script setup lang="ts">
import { computed } from 'vue';
import { PluginItem } from '../stores/plugins';

const props = defineProps<{
  plugin: PluginItem;
  isOpen: boolean;
  loading: boolean;
}>();

const emit = defineEmits<{
  (e: 'close'): void;
  (e: 'confirm'): void;
}>();

const capabilities = computed(() => {
  if (!props.plugin.manifest_json) return null;
  try {
    const m = JSON.parse(props.plugin.manifest_json);
    return m.capabilities || null;
  } catch {
    return null;
  }
});
</script>

<template>
  <div v-if="isOpen" class="modal-overlay" @click.self="emit('close')">
    <div class="modal-card">
      <div class="modal-header">
        <div class="title-group">
          <span class="modal-icon">🛡️</span>
          <div>
            <h3 class="modal-title">プラグインの有効化と権限承認</h3>
            <p class="modal-subtitle">{{ plugin.name }} ({{ plugin.id }})</p>
          </div>
        </div>
        <button class="btn-close" @click="emit('close')">✕</button>
      </div>

      <div class="modal-body">
        <p class="desc-text">
          このプラグインを有効化すると、以下の権限（Capabilities）が付与され、専用プロセスとして起動されます。
        </p>

        <div class="cap-group">
          <div class="cap-title">📋 ジョブ実行権限 (Jobs):</div>
          <div v-if="!capabilities?.jobs?.length" class="cap-empty">なし</div>
          <ul v-else class="cap-list">
            <li v-for="job in capabilities.jobs" :key="job" class="cap-item">
              <span class="cap-badge">job</span>
              <code>{{ job }}</code>
            </li>
          </ul>
        </div>

        <div v-if="capabilities?.systemd_units?.length" class="cap-group">
          <div class="cap-title">⚙️ Systemd ユニット操作権限:</div>
          <ul class="cap-list">
            <li v-for="u in capabilities.systemd_units" :key="u" class="cap-item">
              <span class="cap-badge">systemd</span>
              <code>{{ u }}</code>
            </li>
          </ul>
        </div>

        <div v-if="capabilities?.docker_containers?.length" class="cap-group">
          <div class="cap-title">🐳 Docker コンテナ操作権限:</div>
          <ul class="cap-list">
            <li v-for="c in capabilities.docker_containers" :key="c" class="cap-item">
              <span class="cap-badge">docker</span>
              <code>{{ c }}</code>
            </li>
          </ul>
        </div>

        <div class="security-note">
          ⚠️ プラグインは分離プロセスとして実行されます。信頼できるプラグインのみ有効化してください。
        </div>
      </div>

      <div class="modal-footer">
        <button class="btn btn-secondary" :disabled="loading" @click="emit('close')">
          キャンセル
        </button>
        <button
          class="btn btn-primary"
          :disabled="loading"
          @click="emit('confirm')"
          id="btn-confirm-enable"
        >
          {{ loading ? '起動中...' : '承認して有効化' }}
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
  max-width: 540px;
  display: flex;
  flex-direction: column;
  box-shadow: var(--shadow-modal);
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

.modal-body {
  padding: 1.5rem;
  display: flex;
  flex-direction: column;
  gap: 1.25rem;
}

.desc-text {
  font-size: 0.875rem;
  color: var(--text-base);
  margin: 0;
}

.cap-group {
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
}

.cap-title {
  font-size: 0.875rem;
  font-weight: 600;
  color: var(--text-base);
}

.cap-empty {
  font-size: 0.8rem;
  color: var(--text-muted);
  font-style: italic;
}

.cap-list {
  list-style: none;
  padding: 0;
  margin: 0;
  display: flex;
  flex-direction: column;
  gap: 0.35rem;
}

.cap-item {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  background-color: var(--bg-surface);
  padding: 0.4rem 0.75rem;
  border-radius: var(--radius-md);
  font-size: 0.85rem;
}

.cap-badge {
  background-color: rgba(59, 130, 246, 0.2);
  color: #60a5fa;
  font-size: 0.7rem;
  font-weight: 600;
  padding: 0.1rem 0.35rem;
  border-radius: 4px;
}

.security-note {
  background-color: rgba(234, 179, 8, 0.1);
  border: 1px solid var(--warning);
  color: var(--warning);
  padding: 0.75rem 1rem;
  border-radius: var(--radius-md);
  font-size: 0.8rem;
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

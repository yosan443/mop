<script setup lang="ts">
import { ref } from 'vue';

const props = defineProps<{
  resourceId: string;
  resourceName: string;
  action: 'start' | 'stop' | 'restart';
}>();

const emit = defineEmits<{
  (e: 'confirm'): void;
  (e: 'cancel'): void;
}>();

const loading = ref(false);
const error = ref<string | null>(null);

function actionTitle(action: string) {
  switch (action) {
    case 'start': return '起動 (Start)';
    case 'stop': return '停止 (Stop)';
    case 'restart': return '再起動 (Restart)';
    default: return action;
  }
}

function actionDescription(action: string) {
  switch (action) {
    case 'start': return `リソース「${props.resourceName}」を起動します。よろしいですか？`;
    case 'stop': return `⚠️ リソース「${props.resourceName}」を停止します。サービスが利用不可になります。よろしいですか？`;
    case 'restart': return `リソース「${props.resourceName}」を再起動します。一時的に接続が切断される可能性があります。`;
    default: return `リソース「${props.resourceName}」に対して ${action} を実行します。`;
  }
}

async function handleConfirm() {
  loading.value = true;
  error.value = null;
  emit('confirm');
}
</script>

<template>
  <div class="modal-backdrop" @click.self="emit('cancel')">
    <div class="modal-content card">
      <div class="modal-header">
        <h3 class="modal-title">リソース操作の確認: {{ actionTitle(action) }}</h3>
        <button class="btn-close" @click="emit('cancel')">×</button>
      </div>

      <div class="modal-body">
        <p class="confirm-message">{{ actionDescription(action) }}</p>
        
        <div class="resource-target-box">
          <div class="target-row">
            <span class="target-label">対象 ID:</span>
            <code class="target-val">{{ resourceId }}</code>
          </div>
          <div class="target-row">
            <span class="target-label">対象名:</span>
            <span class="target-val font-semibold">{{ resourceName }}</span>
          </div>
        </div>

        <div v-if="error" class="alert alert-error">
          {{ error }}
        </div>
      </div>

      <div class="modal-footer">
        <button
          type="button"
          class="btn btn-secondary"
          id="btn-cancel-action"
          :disabled="loading"
          @click="emit('cancel')"
        >
          キャンセル
        </button>
        <button
          type="button"
          class="btn"
          :class="action === 'stop' ? 'btn-danger' : 'btn-primary'"
          id="btn-confirm-action"
          :disabled="loading"
          @click="handleConfirm"
        >
          <span v-if="loading" class="spinner"></span>
          <span v-else>{{ actionTitle(action) }} を実行</span>
        </button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.modal-backdrop {
  position: fixed;
  top: 0;
  left: 0;
  width: 100vw;
  height: 100vh;
  background: rgba(0, 0, 0, 0.65);
  backdrop-filter: blur(4px);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 1000;
  padding: 1rem;
}

.modal-content {
  width: 100%;
  max-width: 480px;
  background: var(--color-bg-surface);
  border: 1px solid var(--color-border);
  border-radius: var(--radius-lg);
  box-shadow: var(--shadow-xl);
  overflow: hidden;
}

.modal-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 1.25rem 1.5rem;
  border-bottom: 1px solid var(--color-border);
}

.modal-title {
  margin: 0;
  font-size: 1.15rem;
  font-weight: 600;
  color: var(--color-text-primary);
}

.btn-close {
  background: none;
  border: none;
  font-size: 1.5rem;
  line-height: 1;
  color: var(--color-text-muted);
  cursor: pointer;
  padding: 0.25rem;
  border-radius: var(--radius-sm);
}

.btn-close:hover {
  color: var(--color-text-primary);
  background: var(--color-bg-hover);
}

.modal-body {
  padding: 1.5rem;
  display: flex;
  flex-direction: column;
  gap: 1.25rem;
}

.confirm-message {
  margin: 0;
  font-size: 0.95rem;
  color: var(--color-text-primary);
  line-height: 1.5;
}

.resource-target-box {
  background: var(--color-bg-elevated);
  border: 1px solid var(--color-border);
  border-radius: var(--radius-md);
  padding: 0.85rem 1rem;
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
  font-size: 0.85rem;
}

.target-row {
  display: flex;
  align-items: center;
  gap: 0.5rem;
}

.target-label {
  color: var(--color-text-muted);
  min-width: 60px;
}

.target-val {
  color: var(--color-text-primary);
}

.font-semibold {
  font-weight: 600;
}

.modal-footer {
  display: flex;
  align-items: center;
  justify-content: flex-end;
  gap: 0.75rem;
  padding: 1.25rem 1.5rem;
  border-top: 1px solid var(--color-border);
  background: var(--color-bg-elevated);
}

.btn-danger {
  background: var(--color-error);
  color: white;
  border: none;
}

.btn-danger:hover:not(:disabled) {
  filter: brightness(1.1);
}
</style>

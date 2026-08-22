<script setup lang="ts">
import { computed, ref } from 'vue';

const props = defineProps<{
  resourceId: string;
  resourceName: string;
  action: 'start' | 'stop' | 'restart';
  labelsJson?: string;
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

interface ContainerInfo {
  name: string;
  service?: string;
  status?: string;
  is_managed: boolean;
}

const parsedContainers = computed<ContainerInfo[]>(() => {
  if (!props.labelsJson) return [];
  try {
    const val = JSON.parse(props.labelsJson);
    if (Array.isArray(val.containers)) {
      return val.containers;
    }
  } catch {}
  return [];
});

const managedContainers = computed(() => {
  return parsedContainers.value.filter(c => c.is_managed);
});

const unmanagedContainers = computed(() => {
  return parsedContainers.value.filter(c => !c.is_managed);
});

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

        <!-- Compose Container Scope Detail (SPEC §9.3 & 不変条件 §19) -->
        <div v-if="parsedContainers.length > 0" class="compose-scope-box" id="compose-scope-box">
          <h4 class="scope-title">Compose 操作スコープ (構成コンテナ)</h4>

          <!-- Managed containers (Action Target) -->
          <div class="scope-group managed-scope" id="scope-managed-containers">
            <div class="scope-header">
              <span class="scope-icon">🎯</span>
              <strong>再起動される管理対象コンテナ ({{ managedContainers.length }}):</strong>
            </div>
            <ul class="container-list">
              <li v-for="c in managedContainers" :key="c.name" class="container-item managed-item">
                <span class="badge badge-success-sm">managed</span>
                <span class="container-name font-mono">{{ c.name }}</span>
                <span v-if="c.service" class="service-tag">({{ c.service }})</span>
              </li>
            </ul>
          </div>

          <!-- Unmanaged containers (Protected & Excluded) -->
          <div v-if="unmanagedContainers.length > 0" class="scope-group unmanaged-scope" id="scope-unmanaged-containers">
            <div class="scope-header">
              <span class="scope-icon">🛡️</span>
              <strong>除外される未管理コンテナ (変更なし - {{ unmanagedContainers.length }}):</strong>
            </div>
            <ul class="container-list">
              <li v-for="c in unmanagedContainers" :key="c.name" class="container-item unmanaged-item">
                <span class="badge badge-neutral-sm">unmanaged</span>
                <span class="container-name font-mono">{{ c.name }}</span>
                <span v-if="c.service" class="service-tag">({{ c.service }})</span>
              </li>
            </ul>
            <p class="scope-note">※ <code>mop.managed=true</code> が付与されていないコンテナは保護され、変更されません。</p>
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
  max-width: 520px;
  background: var(--color-bg-surface);
  border: 1px solid var(--color-border);
  border-radius: var(--radius-lg);
  box-shadow: var(--shadow-xl);
  overflow: hidden;
  max-height: 90vh;
  display: flex;
  flex-direction: column;
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
  overflow-y: auto;
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

.compose-scope-box {
  background: var(--color-bg-elevated);
  border: 1px solid var(--color-border);
  border-radius: var(--radius-md);
  padding: 1rem;
  display: flex;
  flex-direction: column;
  gap: 0.85rem;
}

.scope-title {
  margin: 0;
  font-size: 0.9rem;
  font-weight: 600;
  color: var(--color-text-primary);
}

.scope-group {
  display: flex;
  flex-direction: column;
  gap: 0.4rem;
  font-size: 0.85rem;
}

.scope-header {
  display: flex;
  align-items: center;
  gap: 0.4rem;
  color: var(--color-text-primary);
}

.container-list {
  list-style: none;
  padding: 0;
  margin: 0;
  display: flex;
  flex-direction: column;
  gap: 0.35rem;
  padding-left: 1.5rem;
}

.container-item {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  font-size: 0.85rem;
}

.badge-success-sm {
  font-size: 0.7rem;
  padding: 0.1rem 0.4rem;
  background: rgba(16, 185, 129, 0.15);
  color: #10b981;
  border: 1px solid rgba(16, 185, 129, 0.3);
  border-radius: 4px;
}

.badge-neutral-sm {
  font-size: 0.7rem;
  padding: 0.1rem 0.4rem;
  background: rgba(148, 163, 184, 0.15);
  color: #94a3b8;
  border: 1px solid rgba(148, 163, 184, 0.3);
  border-radius: 4px;
}

.service-tag {
  color: var(--color-text-muted);
  font-size: 0.8rem;
}

.scope-note {
  margin: 0.3rem 0 0 1.5rem;
  font-size: 0.75rem;
  color: var(--color-text-muted);
}

.font-semibold {
  font-weight: 600;
}

.font-mono {
  font-family: monospace;
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

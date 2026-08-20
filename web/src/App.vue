<script setup lang="ts">
import { onMounted } from 'vue';
import { useAuthStore } from './stores/auth';

const authStore = useAuthStore();

onMounted(async () => {
  if (!authStore.isInitialized) {
    await authStore.initialize();
  }
});
</script>

<template>
  <div v-if="!authStore.isInitialized" class="global-loading">
    <div class="spinner"></div>
    <div class="loading-text">mop を読み込み中...</div>
  </div>
  <router-view v-else />
</template>

<style scoped>
.global-loading {
  min-height: 100vh;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 1.5rem;
  background-color: var(--bg-app);
}

.spinner {
  width: 40px;
  height: 40px;
  border: 3px solid var(--border-subtle);
  border-top-color: var(--primary);
  border-radius: 50%;
  animation: spin 0.8s linear infinite;
}

.loading-text {
  font-size: 0.875rem;
  color: var(--text-muted);
  font-weight: 500;
}

@keyframes spin {
  to {
    transform: rotate(360deg);
  }
}
</style>

<script setup lang="ts">
import { ref, computed } from 'vue';
import { useRouter } from 'vue-router';
import { useAuthStore } from '../stores/auth';

const router = useRouter();
const authStore = useAuthStore();

const username = ref('');
const password = ref('');
const confirmPassword = ref('');
const errorMessage = ref('');
const isSubmitting = ref(false);

const minPasswordLen = computed(() => authStore.meta?.min_password_len ?? 10);

async function handleSetup() {
  errorMessage.value = '';

  if (!username.value.trim()) {
    errorMessage.value = 'ユーザー名を入力してください';
    return;
  }

  if (password.value.length < minPasswordLen.value) {
    errorMessage.value = `パスワードは ${minPasswordLen.value} 文字以上である必要があります`;
    return;
  }

  if (password.value !== confirmPassword.value) {
    errorMessage.value = 'パスワードが一致しません';
    return;
  }

  isSubmitting.value = true;
  try {
    await authStore.register(username.value.trim(), password.value);
    await router.push('/');
  } catch (err: any) {
    errorMessage.value = err.message || '初期セットアップに失敗しました';
  } finally {
    isSubmitting.value = false;
  }
}
</script>

<template>
  <div class="auth-container">
    <div class="auth-card card">
      <div class="auth-header">
        <div class="logo-circle">
          <svg viewBox="0 0 24 24" width="28" height="28" fill="none" stroke="currentColor" stroke-width="2">
            <path d="M12 2L2 7l10 5 10-5-10-5zM2 17l10 5 10-5M2 12l10 5 10-5" />
          </svg>
        </div>
        <h1 class="auth-title">mop 初期セットアップ</h1>
        <p class="auth-subtitle">最初の管理者 (Admin) アカウントを作成します</p>
      </div>

      <div v-if="errorMessage" class="alert alert-error" id="setup-error">
        <svg viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="2">
          <circle cx="12" cy="12" r="10" />
          <line x1="12" y1="8" x2="12" y2="12" />
          <line x1="12" y1="16" x2="12.01" y2="16" />
        </svg>
        <span>{{ errorMessage }}</span>
      </div>

      <form @submit.prevent="handleSetup">
        <div class="form-group">
          <label class="form-label" for="username">管理者ユーザー名</label>
          <input
            id="username"
            v-model="username"
            type="text"
            class="form-input"
            placeholder="admin"
            required
            autocomplete="username"
            autofocus
          />
        </div>

        <div class="form-group">
          <label class="form-label" for="password">パスワード</label>
          <input
            id="password"
            v-model="password"
            type="password"
            class="form-input"
            :placeholder="`${minPasswordLen}文字以上`"
            required
            autocomplete="new-password"
          />
          <span class="form-help">半角英数記号、{{ minPasswordLen }} 文字以上</span>
        </div>

        <div class="form-group">
          <label class="form-label" for="confirmPassword">パスワード (確認)</label>
          <input
            id="confirmPassword"
            v-model="confirmPassword"
            type="password"
            class="form-input"
            placeholder="パスワードを再入力"
            required
            autocomplete="new-password"
          />
        </div>

        <button
          id="btn-submit-setup"
          type="submit"
          class="btn btn-primary btn-block"
          :disabled="isSubmitting"
        >
          <span v-if="isSubmitting">作成中...</span>
          <span v-else>管理者アカウントを作成して開始</span>
        </button>
      </form>
    </div>
  </div>
</template>

<style scoped>
.auth-container {
  min-height: 100vh;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 1.5rem;
  background: radial-gradient(circle at 50% 30%, #1e293b 0%, #090d16 80%);
}

.auth-card {
  width: 100%;
  max-width: 440px;
  background-color: var(--bg-surface);
  border: 1px solid var(--border-medium);
  box-shadow: var(--shadow-lg);
}

.auth-header {
  text-align: center;
  margin-bottom: 2rem;
}

.logo-circle {
  width: 56px;
  height: 56px;
  margin: 0 auto 1rem;
  border-radius: var(--radius-lg);
  background: linear-gradient(135deg, var(--primary) 0%, #1d4ed8 100%);
  display: flex;
  align-items: center;
  justify-content: center;
  color: white;
  box-shadow: var(--shadow-glow);
}

.auth-title {
  font-size: 1.5rem;
  font-weight: 700;
  letter-spacing: -0.02em;
  margin-bottom: 0.5rem;
}

.auth-subtitle {
  font-size: 0.875rem;
  color: var(--text-muted);
}

.btn-block {
  width: 100%;
  padding: 0.75rem;
  margin-top: 0.75rem;
  font-size: 0.9375rem;
}
</style>

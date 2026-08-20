<script setup lang="ts">
import { ref, computed } from 'vue';
import { useRouter } from 'vue-router';
import { useAuthStore } from '../stores/auth';

const router = useRouter();
const authStore = useAuthStore();

const username = ref('');
const password = ref('');
const errorMessage = ref('');
const isSubmitting = ref(false);

const canRegister = computed(() => authStore.meta?.registration === 'open');

async function handleLogin() {
  errorMessage.value = '';

  if (!username.value.trim() || !password.value) {
    errorMessage.value = 'ユーザー名とパスワードを入力してください';
    return;
  }

  isSubmitting.value = true;
  try {
    await authStore.login(username.value.trim(), password.value);
    await router.push('/');
  } catch (err: any) {
    errorMessage.value = err.message || 'ログインに失敗しました';
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
        <h1 class="auth-title">mop にログイン</h1>
        <p class="auth-subtitle">master-of-process デーモン管理コンソール</p>
      </div>

      <div v-if="errorMessage" class="alert alert-error" id="login-error">
        <svg viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="2">
          <circle cx="12" cy="12" r="10" />
          <line x1="12" y1="8" x2="12" y2="12" />
          <line x1="12" y1="16" x2="12.01" y2="16" />
        </svg>
        <span>{{ errorMessage }}</span>
      </div>

      <form @submit.prevent="handleLogin">
        <div class="form-group">
          <label class="form-label" for="username">ユーザー名</label>
          <input
            id="username"
            v-model="username"
            type="text"
            class="form-input"
            placeholder="ユーザー名を入力"
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
            placeholder="パスワードを入力"
            required
            autocomplete="current-password"
          />
        </div>

        <button
          id="btn-submit-login"
          type="submit"
          class="btn btn-primary btn-block"
          :disabled="isSubmitting"
        >
          <span v-if="isSubmitting">ログイン中...</span>
          <span v-else>ログイン</span>
        </button>
      </form>

      <div v-if="canRegister" class="auth-footer">
        <p>アカウントをお持ちでないですか？ <router-link to="/register">新規登録</router-link></p>
      </div>
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
  max-width: 420px;
  background-color: var(--bg-surface);
  border: 1px solid var(--border-medium);
  box-shadow: var(--shadow-lg);
}

.auth-header {
  text-align: center;
  margin-bottom: 2rem;
}

.logo-circle {
  width: 52px;
  height: 52px;
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
  font-size: 1.375rem;
  font-weight: 700;
  letter-spacing: -0.02em;
  margin-bottom: 0.35rem;
}

.auth-subtitle {
  font-size: 0.8125rem;
  color: var(--text-muted);
}

.btn-block {
  width: 100%;
  padding: 0.75rem;
  margin-top: 0.75rem;
  font-size: 0.9375rem;
}

.auth-footer {
  margin-top: 1.5rem;
  padding-top: 1.25rem;
  border-top: 1px solid var(--border-subtle);
  text-align: center;
  font-size: 0.8125rem;
  color: var(--text-muted);
}
</style>

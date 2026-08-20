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

async function handleRegister() {
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
    errorMessage.value = err.message || 'アカウント作成に失敗しました';
  } finally {
    isSubmitting.value = false;
  }
}
</script>

<template>
  <div class="auth-container">
    <div class="auth-card card">
      <div class="auth-header">
        <h1 class="auth-title">新規ユーザー登録</h1>
        <p class="auth-subtitle">mop アカウントを作成します (既定権限: Viewer)</p>
      </div>

      <div v-if="errorMessage" class="alert alert-error" id="register-error">
        <svg viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="2">
          <circle cx="12" cy="12" r="10" />
          <line x1="12" y1="8" x2="12" y2="12" />
          <line x1="12" y1="16" x2="12.01" y2="16" />
        </svg>
        <span>{{ errorMessage }}</span>
      </div>

      <form @submit.prevent="handleRegister">
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
            :placeholder="`${minPasswordLen}文字以上`"
            required
            autocomplete="new-password"
          />
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
          id="btn-submit-register"
          type="submit"
          class="btn btn-primary btn-block"
          :disabled="isSubmitting"
        >
          <span v-if="isSubmitting">登録中...</span>
          <span v-else>アカウントを作成</span>
        </button>
      </form>

      <div class="auth-footer">
        <p>既にアカウントをお持ちですか？ <router-link to="/login">ログイン</router-link></p>
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

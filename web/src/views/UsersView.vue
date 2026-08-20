<script setup lang="ts">
import { ref, onMounted, computed } from 'vue';
import { useAuthStore, type User, type UserRole } from '../stores/auth';

const authStore = useAuthStore();

const users = ref<User[]>([]);
const isLoading = ref(false);
const errorMessage = ref('');
const successMessage = ref('');

// Create Modal
const showCreateModal = ref(false);
const newUsername = ref('');
const newPassword = ref('');
const newRole = ref<UserRole>('viewer');
const isSubmitting = ref(false);
const modalError = ref('');

const minPasswordLen = computed(() => authStore.meta?.min_password_len ?? 10);
const currentUser = computed(() => authStore.user);

onMounted(async () => {
  await fetchUsers();
});

async function fetchUsers() {
  isLoading.value = true;
  errorMessage.value = '';
  try {
    const res = await fetch('/api/v1/users', {
      credentials: 'include',
    });
    if (!res.ok) {
      const data = await res.json();
      throw new Error(data.error?.message || 'ユーザー一覧の取得に失敗しました');
    }
    users.value = await res.json();
  } catch (err: any) {
    errorMessage.value = err.message;
  } finally {
    isLoading.value = false;
  }
}

async function handleCreateUser() {
  modalError.value = '';
  if (!newUsername.value.trim() || !newPassword.value) {
    modalError.value = 'ユーザー名とパスワードを入力してください';
    return;
  }
  if (newPassword.value.length < minPasswordLen.value) {
    modalError.value = `パスワードは ${minPasswordLen.value} 文字以上である必要があります`;
    return;
  }

  isSubmitting.value = true;
  try {
    const res = await fetch('/api/v1/users', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      credentials: 'include',
      body: JSON.stringify({
        username: newUsername.value.trim(),
        password: newPassword.value,
        role: newRole.value,
      }),
    });

    const data = await res.json();
    if (!res.ok) {
      throw new Error(data.error?.message || 'ユーザー作成に失敗しました');
    }

    successMessage.value = `ユーザー ${data.username} を作成しました`;
    showCreateModal.value = false;
    newUsername.value = '';
    newPassword.value = '';
    newRole.value = 'viewer';
    await fetchUsers();
  } catch (err: any) {
    modalError.value = err.message;
  } finally {
    isSubmitting.value = false;
  }
}

async function handleRoleChange(user: User, role: UserRole) {
  if (user.role === role) return;
  try {
    const res = await fetch(`/api/v1/users/${user.id}`, {
      method: 'PATCH',
      headers: { 'Content-Type': 'application/json' },
      credentials: 'include',
      body: JSON.stringify({ role }),
    });
    if (!res.ok) {
      const data = await res.json();
      throw new Error(data.error?.message || 'ロール変更に失敗しました');
    }
    successMessage.value = `${user.username} のロールを ${role} に変更しました`;
    await fetchUsers();
  } catch (err: any) {
    errorMessage.value = err.message;
  }
}

async function handleToggleDisable(user: User) {
  const disabled = !user.disabled;
  try {
    const res = await fetch(`/api/v1/users/${user.id}`, {
      method: 'PATCH',
      headers: { 'Content-Type': 'application/json' },
      credentials: 'include',
      body: JSON.stringify({ disabled }),
    });
    if (!res.ok) {
      const data = await res.json();
      throw new Error(data.error?.message || '状態変更に失敗しました');
    }
    successMessage.value = `${user.username} を${disabled ? '無効化' : '有効化'}しました`;
    await fetchUsers();
  } catch (err: any) {
    errorMessage.value = err.message;
  }
}
</script>

<template>
  <div class="dashboard-layout">
    <!-- Navigation Header -->
    <header class="navbar">
      <div class="navbar-left">
        <div class="brand">
          <svg viewBox="0 0 24 24" width="22" height="22" fill="none" stroke="currentColor" stroke-width="2.5">
            <path d="M12 2L2 7l10 5 10-5-10-5zM2 17l10 5 10-5M2 12l10 5 10-5" />
          </svg>
          <span class="brand-title">mop</span>
        </div>

        <nav class="nav-links">
          <router-link to="/" class="nav-item">ダッシュボード</router-link>
          <router-link to="/settings/users" class="nav-item active">ユーザー管理</router-link>
        </nav>
      </div>

      <div class="navbar-right">
        <div v-if="currentUser" class="user-info">
          <span class="username">{{ currentUser.username }}</span>
          <span :class="['badge', `badge-${currentUser.role}`]">{{ currentUser.role }}</span>
        </div>
        <router-link to="/" class="btn btn-secondary btn-sm">戻る</router-link>
      </div>
    </header>

    <!-- Main Content -->
    <main class="main-content">
      <div class="content-container">
        <div class="page-header">
          <div>
            <h1 class="page-title">ユーザー管理</h1>
            <p class="page-subtitle">システムユーザーの登録・ロール設定・アクセス制御 (Admin 専用)</p>
          </div>
          <button id="btn-open-create-user" class="btn btn-primary" @click="showCreateModal = true">
            <svg viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="2">
              <line x1="12" y1="5" x2="12" y2="19" />
              <line x1="5" y1="12" x2="19" y2="12" />
            </svg>
            新規ユーザー作成
          </button>
        </div>

        <div v-if="successMessage" class="alert alert-success">
          <svg viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="2">
            <polyline points="20 6 9 17 4 12" />
          </svg>
          <span>{{ successMessage }}</span>
        </div>

        <div v-if="errorMessage" class="alert alert-error">
          <svg viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="2">
            <circle cx="12" cy="12" r="10" />
            <line x1="12" y1="8" x2="12" y2="12" />
            <line x1="12" y1="16" x2="12.01" y2="16" />
          </svg>
          <span>{{ errorMessage }}</span>
        </div>

        <!-- Users Table Card -->
        <div class="card">
          <div class="table-container">
            <table class="table" id="users-table">
              <thead>
                <tr>
                  <th>ユーザー名</th>
                  <th>ロール</th>
                  <th>ステータス</th>
                  <th>作成日時</th>
                  <th>操作</th>
                </tr>
              </thead>
              <tbody>
                <tr v-if="isLoading">
                  <td colspan="5" style="text-align: center; color: var(--text-muted); padding: 2rem;">
                    読み込み中...
                  </td>
                </tr>
                <tr v-else-if="users.length === 0">
                  <td colspan="5" style="text-align: center; color: var(--text-muted); padding: 2rem;">
                    ユーザーが見つかりません
                  </td>
                </tr>
                <tr v-for="u in users" :key="u.id" :id="`user-row-${u.username}`">
                  <td style="font-weight: 600;">
                    {{ u.username }}
                    <span v-if="u.id === currentUser?.id" class="badge badge-viewer" style="margin-left: 0.5rem; font-size: 0.65rem;">自分</span>
                  </td>
                  <td>
                    <select
                      class="form-select role-select"
                      :value="u.role"
                      :disabled="u.id === currentUser?.id"
                      @change="(e) => handleRoleChange(u, (e.target as HTMLSelectElement).value as UserRole)"
                    >
                      <option value="admin">Admin</option>
                      <option value="operator">Operator</option>
                      <option value="viewer">Viewer</option>
                    </select>
                  </td>
                  <td>
                    <span v-if="u.disabled" class="badge badge-danger">無効</span>
                    <span v-else class="badge badge-running">有効</span>
                  </td>
                  <td style="color: var(--text-dim); font-size: 0.8125rem;">
                    {{ new Date(u.created_at).toLocaleString() }}
                  </td>
                  <td>
                    <button
                      v-if="u.id !== currentUser?.id"
                      class="btn btn-secondary btn-sm"
                      :class="u.disabled ? 'btn-primary' : 'btn-danger'"
                      @click="handleToggleDisable(u)"
                    >
                      {{ u.disabled ? '有効化' : '無効化' }}
                    </button>
                  </td>
                </tr>
              </tbody>
            </table>
          </div>
        </div>
      </div>
    </main>

    <!-- Create User Modal -->
    <div v-if="showCreateModal" class="modal-backdrop" @click.self="showCreateModal = false">
      <div class="modal-dialog">
        <div class="modal-header">
          <h3 class="modal-title">新規ユーザーを作成</h3>
          <button class="btn btn-secondary btn-sm" @click="showCreateModal = false">&times;</button>
        </div>

        <form @submit.prevent="handleCreateUser">
          <div class="modal-body">
            <div v-if="modalError" class="alert alert-error">
              <span>{{ modalError }}</span>
            </div>

            <div class="form-group">
              <label class="form-label" for="new-username">ユーザー名</label>
              <input
                id="new-username"
                v-model="newUsername"
                type="text"
                class="form-input"
                placeholder="operator_user"
                required
                autofocus
              />
            </div>

            <div class="form-group">
              <label class="form-label" for="new-password">パスワード</label>
              <input
                id="new-password"
                v-model="newPassword"
                type="password"
                class="form-input"
                :placeholder="`${minPasswordLen}文字以上`"
                required
              />
              <span class="form-help">半角英数記号、{{ minPasswordLen }} 文字以上</span>
            </div>

            <div class="form-group">
              <label class="form-label" for="new-role">ロール (権限)</label>
              <select id="new-role" v-model="newRole" class="form-select">
                <option value="viewer">Viewer (参照・ログ閲覧のみ)</option>
                <option value="operator">Operator (リソース再起動・ジョブ実行)</option>
                <option value="admin">Admin (全権限・ユーザー管理)</option>
              </select>
            </div>
          </div>

          <div class="modal-footer">
            <button type="button" class="btn btn-secondary" @click="showCreateModal = false">
              キャンセル
            </button>
            <button id="btn-submit-new-user" type="submit" class="btn btn-primary" :disabled="isSubmitting">
              <span v-if="isSubmitting">作成中...</span>
              <span v-else>作成する</span>
            </button>
          </div>
        </form>
      </div>
    </div>
  </div>
</template>

<style scoped>
.dashboard-layout {
  min-height: 100vh;
  display: flex;
  flex-direction: column;
}

.navbar {
  height: 64px;
  background-color: var(--bg-glass);
  backdrop-filter: blur(12px);
  border-bottom: 1px solid var(--border-subtle);
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 0 1.5rem;
  position: sticky;
  top: 0;
  z-index: 50;
}

.navbar-left, .navbar-right {
  display: flex;
  align-items: center;
  gap: 1.25rem;
}

.brand {
  display: flex;
  align-items: center;
  gap: 0.6rem;
  font-weight: 700;
  font-size: 1.25rem;
  color: var(--text-main);
}

.nav-links {
  display: flex;
  gap: 0.5rem;
  margin-left: 1rem;
}

.nav-item {
  padding: 0.4rem 0.8rem;
  border-radius: var(--radius-md);
  font-size: 0.875rem;
  font-weight: 500;
  color: var(--text-muted);
  transition: all var(--transition-fast);
}

.nav-item:hover {
  color: var(--text-main);
  background-color: var(--bg-surface-raised);
  text-decoration: none;
}

.nav-item.active {
  color: var(--text-main);
  background-color: var(--primary-surface);
  color: var(--primary);
}

.user-info {
  display: flex;
  align-items: center;
  gap: 0.6rem;
}

.username {
  font-weight: 600;
  font-size: 0.875rem;
}

.main-content {
  flex: 1;
  padding: 2rem 1.5rem;
}

.content-container {
  max-width: 1000px;
  margin: 0 auto;
}

.page-header {
  display: flex;
  justify-content: space-between;
  align-items: flex-end;
  margin-bottom: 1.75rem;
}

.page-title {
  font-size: 1.75rem;
  font-weight: 700;
  letter-spacing: -0.02em;
}

.page-subtitle {
  font-size: 0.875rem;
  color: var(--text-muted);
  margin-top: 0.25rem;
}

.role-select {
  padding: 0.35rem 0.65rem;
  font-size: 0.8125rem;
  width: auto;
}
</style>

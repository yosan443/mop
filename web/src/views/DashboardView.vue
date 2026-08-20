<script setup lang="ts">
import { ref, onMounted, computed } from 'vue';
import { useRouter } from 'vue-router';
import { useAuthStore } from '../stores/auth';

const router = useRouter();
const authStore = useAuthStore();

const healthStatus = ref<'ok' | 'error' | 'loading'>('loading');
const currentTheme = ref<'dark' | 'light'>('dark');

const user = computed(() => authStore.user);
const isAdmin = computed(() => authStore.user?.role === 'admin');

onMounted(async () => {
  try {
    const res = await fetch('/health');
    if (res.ok) {
      healthStatus.value = 'ok';
    } else {
      healthStatus.value = 'error';
    }
  } catch {
    healthStatus.value = 'error';
  }
});

function toggleTheme() {
  currentTheme.value = currentTheme.value === 'dark' ? 'light' : 'dark';
  document.documentElement.setAttribute('data-theme', currentTheme.value);
}

async function handleLogout() {
  await authStore.logout();
  router.push('/login');
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
          <router-link to="/" class="nav-item active">ダッシュボード</router-link>
          <router-link v-if="isAdmin" to="/settings/users" class="nav-item" id="nav-users">ユーザー管理</router-link>
        </nav>
      </div>

      <div class="navbar-right">
        <button class="btn btn-secondary btn-sm" @click="toggleTheme" title="テーマ切り替え">
          <svg v-if="currentTheme === 'dark'" viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="2">
            <circle cx="12" cy="12" r="5" />
            <line x1="12" y1="1" x2="12" y2="3" />
            <line x1="12" y1="21" x2="12" y2="23" />
            <line x1="4.22" y1="4.22" x2="5.64" y2="5.64" />
            <line x1="18.36" y1="18.36" x2="19.78" y2="19.78" />
            <line x1="1" y1="12" x2="3" y2="12" />
            <line x1="21" y1="12" x2="23" y2="12" />
            <line x1="4.22" y1="19.78" x2="5.64" y2="18.36" />
            <line x1="18.36" y1="5.64" x2="19.78" y2="4.22" />
          </svg>
          <svg v-else viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="2">
            <path d="M21 12.79A9 9 0 1 1 11.21 3 7 7 0 0 0 21 12.79z" />
          </svg>
        </button>

        <div v-if="user" class="user-info">
          <span class="username" id="current-username">{{ user.username }}</span>
          <span :class="['badge', `badge-${user.role}`]" id="current-user-role">{{ user.role }}</span>
        </div>

        <button id="btn-logout" class="btn btn-secondary btn-sm" @click="handleLogout">
          ログアウト
        </button>
      </div>
    </header>

    <!-- Main Content -->
    <main class="main-content">
      <div class="content-container">
        <div class="page-header">
          <div>
            <h1 class="page-title">システム概要</h1>
            <p class="page-subtitle">master-of-process コア骨格 (Milestone M1)</p>
          </div>
          <div class="health-indicator">
            <span class="status-dot" :class="healthStatus"></span>
            <span class="status-text">
              {{ healthStatus === 'ok' ? 'デーモン稼働中' : healthStatus === 'loading' ? '確認中...' : '接続エラー' }}
            </span>
          </div>
        </div>

        <!-- Metric Cards -->
        <div class="grid-cards">
          <div class="card metric-card">
            <div class="metric-label">認証セッション</div>
            <div class="metric-value">有効</div>
            <div class="metric-meta">HttpOnly / SameSite Lax Cookie</div>
          </div>

          <div class="card metric-card">
            <div class="metric-label">データベース</div>
            <div class="metric-value">SQLite WAL</div>
            <div class="metric-meta">スキーマ v1 適用済み</div>
          </div>

          <div class="card metric-card">
            <div class="metric-label">PWA アプリケーション</div>
            <div class="metric-value">スタンドアロン</div>
            <div class="metric-meta">Service Worker キャッシュ有効</div>
          </div>
        </div>

        <!-- Milestone M1 Summary Box -->
        <div class="card welcome-card">
          <div class="welcome-header">
            <div class="welcome-icon">
              <svg viewBox="0 0 24 24" width="24" height="24" fill="none" stroke="currentColor" stroke-width="2">
                <path d="M22 11.08V12a10 10 0 1 1-5.93-9.14" />
                <polyline points="22 4 12 14.01 9 11.01" />
              </svg>
            </div>
            <div>
              <h2 class="welcome-title">M1 コア骨格が稼働しています</h2>
              <p class="welcome-desc">
                認証、ユーザー管理、SQLite WAL マイグレーション、SPA 配信、PWA マニフェスト、およびテスト用 Fake バックエンドが正常に構成されています。
              </p>
            </div>
          </div>

          <div class="features-list">
            <div class="feature-item">
              <span class="badge badge-running">M1 完了</span>
              <span>セキュア認証 & RBAC (Admin / Operator / Viewer)</span>
            </div>
            <div class="feature-item">
              <span class="badge badge-running">M1 完了</span>
              <span>管理用 REST API (/api/v1/auth, /api/v1/users, /health)</span>
            </div>
            <div class="feature-item">
              <span class="badge badge-running">M1 完了</span>
              <span>Playwright 2層 E2E テストハーネス</span>
            </div>
            <div class="feature-item">
              <span class="badge badge-stopped">M2 準備中</span>
              <span>systemd (zbus) / Docker (bollard) リアルタイム監視・操作</span>
            </div>
          </div>
        </div>
      </div>
    </main>
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
  max-width: 1100px;
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

.health-indicator {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  background-color: var(--bg-surface);
  border: 1px solid var(--border-subtle);
  padding: 0.4rem 0.8rem;
  border-radius: var(--radius-full);
}

.status-dot {
  width: 10px;
  height: 10px;
  border-radius: 50%;
}

.status-dot.ok {
  background-color: var(--success);
  box-shadow: 0 0 8px var(--success);
}

.status-dot.loading {
  background-color: var(--warning);
}

.status-dot.error {
  background-color: var(--danger);
}

.status-text {
  font-size: 0.8125rem;
  font-weight: 500;
}

.grid-cards {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(280px, 1fr));
  gap: 1.25rem;
  margin-bottom: 1.5rem;
}

.metric-card {
  display: flex;
  flex-direction: column;
}

.metric-label {
  font-size: 0.8125rem;
  font-weight: 600;
  color: var(--text-muted);
  text-transform: uppercase;
  letter-spacing: 0.05em;
}

.metric-value {
  font-size: 1.5rem;
  font-weight: 700;
  margin: 0.5rem 0 0.25rem;
  color: var(--text-main);
}

.metric-meta {
  font-size: 0.75rem;
  color: var(--text-dim);
}

.welcome-card {
  margin-top: 1rem;
}

.welcome-header {
  display: flex;
  gap: 1rem;
  align-items: flex-start;
  margin-bottom: 1.5rem;
}

.welcome-icon {
  width: 44px;
  height: 44px;
  border-radius: var(--radius-md);
  background-color: var(--success-surface);
  color: var(--success);
  display: flex;
  align-items: center;
  justify-content: center;
  flex-shrink: 0;
}

.welcome-title {
  font-size: 1.25rem;
  font-weight: 600;
  margin-bottom: 0.25rem;
}

.welcome-desc {
  font-size: 0.875rem;
  color: var(--text-muted);
}

.features-list {
  display: flex;
  flex-direction: column;
  gap: 0.75rem;
  border-top: 1px solid var(--border-subtle);
  padding-top: 1.25rem;
}

.feature-item {
  display: flex;
  align-items: center;
  gap: 1rem;
  font-size: 0.875rem;
}
</style>

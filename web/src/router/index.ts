import { createRouter, createWebHistory, type RouteRecordRaw } from 'vue-router';
import { useAuthStore } from '../stores/auth';
import DashboardView from '../views/DashboardView.vue';
import ResourceDetailView from '../views/ResourceDetailView.vue';
import LoginView from '../views/LoginView.vue';
import SetupView from '../views/SetupView.vue';
import RegisterView from '../views/RegisterView.vue';
import UsersView from '../views/UsersView.vue';
import PluginsView from '../views/PluginsView.vue';
import PluginContainerView from '../views/PluginContainerView.vue';
import JobsView from '../views/JobsView.vue';

const routes: RouteRecordRaw[] = [
  {
    path: '/',
    name: 'dashboard',
    component: DashboardView,
    meta: { requiresAuth: true },
  },
  {
    path: '/resources/:id',
    name: 'resource-detail',
    component: ResourceDetailView,
    meta: { requiresAuth: true },
  },
  {
    path: '/setup',
    name: 'setup',
    component: SetupView,
  },
  {
    path: '/login',
    name: 'login',
    component: LoginView,
  },
  {
    path: '/register',
    name: 'register',
    component: RegisterView,
  },
  {
    path: '/settings/users',
    name: 'users',
    component: UsersView,
    meta: { requiresAuth: true, requiresAdmin: true },
  },
  {
    path: '/jobs',
    name: 'jobs',
    component: JobsView,
    meta: { requiresAuth: true },
  },
  {
    path: '/plugins',
    name: 'plugins',
    component: PluginsView,
    meta: { requiresAuth: true },
  },
  {
    path: '/plugins/:id',
    name: 'plugin-container',
    component: PluginContainerView,
    meta: { requiresAuth: true },
  },
  {
    path: '/:pathMatch(.*)*',
    redirect: '/',
  },
];

const router = createRouter({
  history: createWebHistory(),
  routes,
});

router.beforeEach(async (to) => {
  const authStore = useAuthStore();

  if (!authStore.isInitialized) {
    await authStore.initialize();
  }

  const needsSetup = authStore.meta?.needs_setup ?? false;
  const isAuthenticated = !!authStore.user;
  const isAdmin = authStore.user?.role === 'admin';

  if (needsSetup) {
    if (to.path !== '/setup') {
      return { path: '/setup' };
    }
    return true;
  }

  // Setup is finished
  if (to.path === '/setup') {
    return isAuthenticated ? { path: '/' } : { path: '/login' };
  }

  if (to.meta.requiresAuth && !isAuthenticated) {
    return { path: '/login' };
  }

  if ((to.path === '/login' || to.path === '/register') && isAuthenticated) {
    return { path: '/' };
  }

  if (to.meta.requiresAdmin && !isAdmin) {
    return { path: '/' };
  }

  return true;
});

export default router;

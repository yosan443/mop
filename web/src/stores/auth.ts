import { defineStore } from 'pinia';
import { ref } from 'vue';

export type UserRole = 'admin' | 'operator' | 'viewer';

export interface User {
  id: string;
  username: string;
  role: UserRole;
  disabled: boolean;
  created_at: string;
  updated_at: string;
}

export interface AuthMeta {
  needs_setup: boolean;
  registration: 'first_user' | 'open' | 'closed';
  min_password_len: number;
  is_fake_backend?: boolean;
}

export const useAuthStore = defineStore('auth', () => {
  const user = ref<User | null>(null);
  const meta = ref<AuthMeta | null>(null);
  const isInitialized = ref(false);
  const isLoading = ref(false);
  const error = ref<string | null>(null);

  async function fetchMeta(): Promise<AuthMeta> {
    const res = await fetch('/api/v1/auth/meta', {
      credentials: 'include',
    });
    if (!res.ok) {
      throw new Error('Failed to fetch auth metadata');
    }
    const data: AuthMeta = await res.json();
    meta.value = data;
    return data;
  }

  async function fetchMe(): Promise<User | null> {
    try {
      const res = await fetch('/api/v1/auth/me', {
        credentials: 'include',
      });
      if (res.status === 401) {
        user.value = null;
        return null;
      }
      if (!res.ok) {
        throw new Error('Failed to fetch user session');
      }
      const data: User = await res.json();
      user.value = data;
      return data;
    } catch {
      user.value = null;
      return null;
    }
  }

  async function initialize(): Promise<void> {
    isLoading.value = true;
    try {
      await fetchMeta();
      await fetchMe();
    } finally {
      isInitialized.value = true;
      isLoading.value = false;
    }
  }

  async function login(username: string, password: string): Promise<User> {
    error.value = null;
    const res = await fetch('/api/v1/auth/login', {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
      },
      credentials: 'include',
      body: JSON.stringify({ username, password }),
    });

    const data = await res.json();
    if (!res.ok) {
      const msg = data.error?.message || 'Login failed';
      error.value = msg;
      throw new Error(msg);
    }

    user.value = data;
    if (meta.value) {
      meta.value.needs_setup = false;
    }
    return data;
  }

  async function register(username: string, password: string): Promise<User> {
    error.value = null;
    const res = await fetch('/api/v1/auth/register', {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
      },
      credentials: 'include',
      body: JSON.stringify({ username, password }),
    });

    const data = await res.json();
    if (!res.ok) {
      const msg = data.error?.message || 'Registration failed';
      error.value = msg;
      throw new Error(msg);
    }

    user.value = data;
    if (meta.value) {
      meta.value.needs_setup = false;
    }
    return data;
  }

  async function logout(): Promise<void> {
    await fetch('/api/v1/auth/logout', {
      method: 'POST',
      credentials: 'include',
    });
    user.value = null;
    await fetchMeta();
  }

  return {
    user,
    meta,
    isInitialized,
    isLoading,
    error,
    initialize,
    fetchMeta,
    fetchMe,
    login,
    register,
    logout,
  };
});

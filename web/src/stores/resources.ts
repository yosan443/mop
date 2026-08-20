import { defineStore } from 'pinia';
import { ref } from 'vue';

export interface Resource {
  id: string;
  kind: 'systemd_unit' | 'docker_container' | 'compose_service' | 'compose_project';
  name: string;
  display_name?: string;
  group_name?: string;
  source: string;
  labels_json?: string;
  first_seen: string;
  last_seen: string;
}

export interface ResourceDetail {
  resource: Resource;
  status: 'running' | 'stopped' | 'failed' | 'degraded' | 'restarting' | 'unknown';
  active_state: string;
  sub_state?: string;
  uptime_secs?: number;
  memory_bytes?: number;
  cpu_percent?: number;
}

export interface ResourceEvent {
  id: string;
  kind: string;
  status: 'running' | 'stopped' | 'failed' | 'degraded' | 'restarting' | 'unknown';
  ts: string;
  message?: string;
}

export const useResourceStore = defineStore('resources', () => {
  const resources = ref<Resource[]>([]);
  const details = ref<Record<string, ResourceDetail>>({});
  const loading = ref(false);
  const error = ref<string | null>(null);
  let eventSource: EventSource | null = null;

  async function fetchResources() {
    loading.value = true;
    error.value = null;
    try {
      const res = await fetch('/api/v1/resources', { credentials: 'include' });
      if (!res.ok) {
        throw new Error(`Failed to fetch resources: ${res.statusText}`);
      }
      resources.value = await res.json();
      // Also fetch details for each resource
      await Promise.allSettled(resources.value.map(r => fetchResourceDetail(r.id)));
    } catch (e: any) {
      error.value = e.message;
    } finally {
      loading.value = false;
    }
  }

  async function fetchResourceDetail(id: string): Promise<ResourceDetail | null> {
    try {
      const res = await fetch(`/api/v1/resources/${encodeURIComponent(id)}`, { credentials: 'include' });
      if (!res.ok) return null;
      const data: ResourceDetail = await res.json();
      details.value[id] = data;
      return data;
    } catch {
      return null;
    }
  }

  async function executeAction(id: string, action: string): Promise<{ job_id: string; status: string }> {
    const res = await fetch(`/api/v1/resources/${encodeURIComponent(id)}/actions`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      credentials: 'include',
      body: JSON.stringify({ action }),
    });

    if (!res.ok) {
      const err = await res.json().catch(() => ({ error: { message: 'Action failed' } }));
      throw new Error(err.error?.message || `Action failed with ${res.status}`);
    }

    const data = await res.json();
    // Temporarily set status to restarting if action was restart
    if (details.value[id] && action === 'restart') {
      details.value[id].status = 'restarting';
    }
    return data;
  }

  function connectEvents() {
    if (eventSource) return;

    eventSource = new EventSource('/api/v1/events/stream');
    eventSource.addEventListener('resource_event', (e) => {
      try {
        const evt: ResourceEvent = JSON.parse(e.data);
        if (details.value[evt.id]) {
          details.value[evt.id].status = evt.status;
        }
      } catch (err) {
        console.error('Failed to parse resource event:', err);
      }
    });

    eventSource.onerror = () => {
      // Reconnect handled automatically by EventSource
    };
  }

  function disconnectEvents() {
    if (eventSource) {
      eventSource.close();
      eventSource = null;
    }
  }

  return {
    resources,
    details,
    loading,
    error,
    fetchResources,
    fetchResourceDetail,
    executeAction,
    connectEvents,
    disconnectEvents,
  };
});

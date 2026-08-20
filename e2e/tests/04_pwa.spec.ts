import { test, expect } from '@playwright/test';

test.describe('PWA & Health Endpoints', () => {
  test('serves health check and valid PWA webmanifest', async ({ request }) => {
    // 1. Health check endpoint
    const healthRes = await request.get('/health');
    expect(healthRes.ok()).toBeTruthy();
    const healthJson = await healthRes.json();
    expect(healthJson.status).toBe('ok');

    // 2. Webmanifest endpoint
    const manifestRes = await request.get('/manifest.webmanifest');
    expect(manifestRes.ok()).toBeTruthy();
    const manifestJson = await manifestRes.json();
    expect(manifestJson.name).toBe('mop');
    expect(manifestJson.display).toBe('standalone');
    expect(manifestJson.icons.length).toBeGreaterThan(0);
  });
});

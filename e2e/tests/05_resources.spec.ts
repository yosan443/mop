import { test, expect } from '@playwright/test';

test.use({ extraHTTPHeaders: { 'x-forwarded-for': '10.0.0.55' } });

test.describe('Dashboard Resource Cards & Grouping (M3)', () => {
  test('displays resource cards, compose project hierarchy, unmanaged badge, and navigates to detail', async ({ page }) => {
    // 1. Initial setup or login
    await page.goto('/');
    await page.waitForLoadState('networkidle');

    if (page.url().includes('/setup')) {
      await page.fill('#username', 'admin');
      await page.fill('#password', 'AdminSecretPassword123!');
      await page.fill('#confirmPassword', 'AdminSecretPassword123!');
      await page.click('#btn-submit-setup');
      await page.waitForURL((url) => url.pathname === '/');
    } else if (page.url().includes('/login')) {
      await page.fill('#username', 'admin');
      await page.fill('#password', 'AdminSecretPassword123!');
      await page.click('#btn-submit-login');
      await page.waitForURL((url) => url.pathname === '/');
    }

    // 2. Verify summary cards
    await expect(page.locator('#stat-total')).toHaveText('8');
    await expect(page.locator('#stat-running')).toHaveText('7');
    await expect(page.locator('#stat-stopped')).toHaveText('1');

    // 3. Verify Systemd and Docker cards
    const caddyCard = page.locator('#resource-card-systemd-caddy-service');
    await expect(caddyCard).toBeVisible();
    await expect(caddyCard).toContainText('Caddy Web Server');

    const komgaCard = page.locator('#resource-card-docker-komga');
    await expect(komgaCard).toBeVisible();
    await expect(komgaCard).toContainText('Komga Media Server');

    // 4. Verify Compose Projects section and hierarchical service display
    const composeSection = page.locator('#section-compose-projects');
    await expect(composeSection).toBeVisible();
    await expect(composeSection).toContainText('Docker Compose Projects');

    const mediaStackProject = page.locator('#resource-card-compose_project-media-stack');
    await expect(mediaStackProject).toBeVisible();
    await expect(mediaStackProject).toContainText('media-stack');
    await expect(mediaStackProject).toContainText('1/2 managed');

    // Verify services under media-stack
    const mangaWorkerService = page.locator('#resource-card-compose_service-media-stack-manga-worker');
    await expect(mangaWorkerService).toBeVisible();
    await expect(mangaWorkerService).toContainText('manga-worker');
    await expect(mangaWorkerService).toContainText('depends_on:');
    await expect(mangaWorkerService).toContainText('db');
    await expect(mangaWorkerService.locator('#btn-restart-compose_service-media-stack-manga-worker')).toBeVisible();

    const dbService = page.locator('#resource-card-compose_service-media-stack-db');
    await expect(dbService).toBeVisible();
    await expect(dbService).toContainText('db');
    await expect(dbService.locator('#badge-unmanaged-compose_service-media-stack-db')).toBeVisible();
    await expect(dbService.locator('#btn-restart-compose_service-media-stack-db')).not.toBeVisible();

    // 5. Navigate to compose service detail
    await page.click('#btn-detail-compose_service-media-stack-manga-worker');
    await page.waitForURL((url) => url.pathname.includes('/resources/compose_service:media-stack:manga-worker') || url.pathname.includes('/resources/compose_service%3Amedia-stack%3Amanga-worker'));

    // Verify detail page components (depends_on, constituent containers, managed status)
    await expect(page.locator('#detail-resource-name')).toContainText('manga-worker');
    await expect(page.locator('#detail-status-badge')).toHaveText('running');
    await expect(page.locator('#badge-managed-status')).toBeVisible();
    await expect(page.locator('#row-depends-on')).toContainText('db');
    await expect(page.locator('#block-containers-list')).toContainText('media-stack-manga-worker-1');
  });
});

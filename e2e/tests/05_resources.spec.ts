import { test, expect } from '@playwright/test';

test.use({ extraHTTPHeaders: { 'x-forwarded-for': '10.0.0.55' } });

test.describe('Dashboard Resource Cards & Grouping (M2)', () => {
  test('displays resource cards, summary statistics, and navigates to detail', async ({ page }) => {
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

    // 3. Verify resource cards exist
    const caddyCard = page.locator('#resource-card-systemd-caddy-service');
    await expect(caddyCard).toBeVisible();
    await expect(caddyCard).toContainText('Caddy Web Server');

    const nginxCard = page.locator('#resource-card-systemd-nginx-service');
    await expect(nginxCard).toBeVisible();
    await expect(nginxCard).toContainText('Nginx Reverse Proxy');

    const komgaCard = page.locator('#resource-card-docker-komga');
    await expect(komgaCard).toBeVisible();
    await expect(komgaCard).toContainText('Komga Media Server');

    // 4. Click detail button on caddy card
    await page.click('#btn-detail-systemd-caddy-service');
    await page.waitForURL((url) => url.pathname.includes('/resources/systemd:caddy.service') || url.pathname.includes('/resources/systemd%3Acaddy.service'));

    // 5. Verify resource detail page
    await expect(page.locator('#detail-resource-name')).toHaveText('Caddy Web Server');
    await expect(page.locator('#detail-status-badge')).toHaveText('running');
    await expect(page.locator('#metric-uptime')).not.toBeEmpty();
  });
});

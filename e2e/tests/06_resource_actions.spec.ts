import { test, expect } from '@playwright/test';

test.use({ extraHTTPHeaders: { 'x-forwarded-for': '10.0.0.66' } });

test.describe('Resource Actions & Confirmation Modal (M2)', () => {
  test('triggers resource restart via confirmation modal and observes state transitions', async ({ page }) => {
    // 1. Ensure logged in as admin
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

    // 2. Click restart on Caddy service card
    const caddyCard = page.locator('#resource-card-systemd-caddy-service');
    await expect(caddyCard).toBeVisible();
    await page.click('#btn-restart-systemd-caddy-service');

    // 3. Verify confirmation modal appears
    const modal = page.locator('.modal-content');
    await expect(modal).toBeVisible();
    await expect(modal).toContainText('リソース操作の確認');
    await expect(modal).toContainText('systemd:caddy.service');

    // 4. Confirm restart
    await page.click('#btn-confirm-action');

    // 5. Verify modal closes and action success banner appears
    await expect(modal).not.toBeVisible();
    await expect(page.locator('.alert-success')).toBeVisible();
    await expect(page.locator('.alert-success')).toContainText('ジョブを受け付けました');

    // 6. Navigate to detail view and assert status transitions from restarting -> running
    await page.click('#btn-detail-systemd-caddy-service');
    await page.waitForURL((url) => url.pathname.includes('/resources/systemd:caddy.service') || url.pathname.includes('/resources/systemd%3Acaddy.service'));

    const statusBadge = page.locator('#detail-status-badge');
    await expect(statusBadge).toHaveText(/running|restarting/);
    await expect(statusBadge).toHaveText('running', { timeout: 10000 });
  });
});

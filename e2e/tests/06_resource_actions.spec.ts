import { test, expect } from '@playwright/test';

test.use({ extraHTTPHeaders: { 'x-forwarded-for': '10.0.0.66' } });

test.describe('Resource Actions & Confirmation Modal (M3)', () => {
  test('triggers compose project restart via confirmation modal with managed/unmanaged container scope', async ({ page }) => {
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

    // 2. Click restart on Compose project (media-stack)
    const projCard = page.locator('#resource-card-compose_project-media-stack');
    await expect(projCard).toBeVisible();
    await page.click('#btn-restart-compose_project-media-stack');

    // 3. Verify confirmation modal displays managed and unmanaged container scope
    const modal = page.locator('.modal-content');
    await expect(modal).toBeVisible();
    await expect(modal).toContainText('リソース操作の確認');
    await expect(modal).toContainText('compose_project:media-stack');

    // Verify scope block
    const scopeBox = modal.locator('#compose-scope-box');
    await expect(scopeBox).toBeVisible();
    await expect(scopeBox.locator('#scope-managed-containers')).toContainText('media-stack-manga-worker-1');
    await expect(scopeBox.locator('#scope-unmanaged-containers')).toContainText('media-stack-db-1');
    await expect(scopeBox.locator('#scope-unmanaged-containers')).toContainText('保護');

    // 4. Confirm restart
    await page.click('#btn-confirm-action');

    // 5. Verify modal closes and action success banner appears
    await expect(modal).not.toBeVisible();
    await expect(page.locator('.alert-success')).toBeVisible();
    await expect(page.locator('.alert-success')).toContainText('ジョブを受け付けました');

    // 6. Navigate to detail view and assert status transitions from restarting -> running
    await page.click('#btn-detail-compose_project-media-stack');
    await page.waitForURL((url) => url.pathname.includes('/resources/compose_project:media-stack') || url.pathname.includes('/resources/compose_project%3Amedia-stack'));

    const statusBadge = page.locator('#detail-status-badge');
    await expect(statusBadge).toHaveText(/running|restarting/);
    await expect(statusBadge).toHaveText('running', { timeout: 10000 });
  });
});

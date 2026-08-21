import { test, expect } from '@playwright/test';

test.use({ extraHTTPHeaders: { 'x-forwarded-for': '10.0.0.7' } });

test.describe('Live Log Viewer & SSE Stream (M2)', () => {
  test('displays real-time log lines and responds to filter and search', async ({ page }) => {
    // 1. Log in
    await page.goto('/');
    await page.waitForLoadState('networkidle');

    if (page.url().includes('/setup')) {
      await page.fill('#username', 'admin');
      await page.fill('#password', 'AdminSecretPassword123!');
      await page.fill('#confirmPassword', 'AdminSecretPassword123!');
      await page.click('#btn-submit-setup');
      await page.waitForURL('http://127.0.0.1:18999/');
    } else if (page.url().includes('/login')) {
      await page.fill('#username', 'admin');
      await page.fill('#password', 'AdminSecretPassword123!');
      await page.click('#btn-submit-login');
      await page.waitForURL('http://127.0.0.1:18999/');
    }

    // 2. Navigate to resource detail
    await page.goto('/resources/systemd%3Acaddy.service');
    await page.waitForLoadState('networkidle');

    // 3. Verify Live Stream badge is connected
    const badge = page.locator('.connection-badge');
    await expect(badge).toBeVisible();
    await expect(badge).toContainText('LIVE STREAM');

    // 4. Verify terminal rows exist
    const terminal = page.locator('#log-terminal-container');
    await expect(terminal).toBeVisible();
    await expect(terminal.locator('.log-row')).not.toHaveCount(0);

    // 5. Test search filter
    await page.fill('#log-search-input', 'initialized');
    await expect(terminal.locator('.log-row')).toContainText(['initialized']);

    // 6. Test autoscroll toggle
    const autoScrollBtn = page.locator('#btn-toggle-autoscroll');
    await expect(autoScrollBtn).toContainText('自動スクロール: ON');
    await autoScrollBtn.click();
    await expect(autoScrollBtn).toContainText('自動スクロール: OFF');
  });
});

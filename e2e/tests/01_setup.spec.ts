import { test, expect } from '@playwright/test';

test.use({ extraHTTPHeaders: { 'x-forwarded-for': '10.0.0.1' } });

test.describe('Initial Setup Flow', () => {
  test('redirects to /setup and creates initial admin account', async ({ page }) => {
    // 1. First visit to / should redirect to /setup when no users exist
    await page.goto('/');
    await expect(page).toHaveURL('http://127.0.0.1:18999/setup');

    // 2. Verify setup form
    await expect(page.locator('#username')).toBeVisible();
    await expect(page.locator('#password')).toBeVisible();
    await expect(page.locator('#confirmPassword')).toBeVisible();

    // 3. Fill and submit setup form
    await page.fill('#username', 'admin');
    await page.fill('#password', 'AdminSecretPassword123!');
    await page.fill('#confirmPassword', 'AdminSecretPassword123!');
    await page.click('#btn-submit-setup');

    // 4. Should redirect to dashboard
    await expect(page).toHaveURL('http://127.0.0.1:18999/');
    await expect(page.locator('#current-username')).toContainText('admin');
    await expect(page.locator('#current-user-role')).toContainText('admin');

    // 5. Subsequent access to /setup should redirect to /
    await page.goto('/setup');
    await expect(page).toHaveURL('http://127.0.0.1:18999/');
  });
});

import { test, expect } from '@playwright/test';

test.use({ extraHTTPHeaders: { 'x-forwarded-for': '10.0.0.2' } });

test.describe('Authentication Flow', () => {
  test('handles invalid credentials, login, session persistence, and logout', async ({ page }) => {
    // 1. Visit login (if already setup from previous test)
    await page.goto('/login');
    await page.waitForLoadState('networkidle');

    // 2. Attempt login with wrong credentials
    await page.fill('#username', 'admin');
    await page.fill('#password', 'WrongPassword123!');
    await page.click('#btn-submit-login');
    await expect(page.locator('.alert-error')).toBeVisible();

    // 3. Login with correct credentials
    await page.fill('#password', 'AdminSecretPassword123!');
    await page.click('#btn-submit-login');

    await expect(page).toHaveURL('http://127.0.0.1:18999/');
    await expect(page.locator('#current-username')).toContainText('admin');
    await expect(page.locator('#current-user-role')).toContainText('admin');

    // 4. Session persistence after reload
    await page.reload();
    await expect(page).toHaveURL('http://127.0.0.1:18999/');
    await expect(page.locator('#current-username')).toContainText('admin');

    // 5. Logout
    await page.click('#btn-logout');
    await expect(page).toHaveURL(/.*\/login/);

    // 6. Accessing / while logged out redirects to /login
    await page.goto('/');
    await expect(page).toHaveURL(/.*\/login/);
  });
});

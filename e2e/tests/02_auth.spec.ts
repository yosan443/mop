import { test, expect } from '@playwright/test';

test.describe('Authentication Flow', () => {
  test('handles invalid credentials, login, session persistence, and logout', async ({ page }) => {
    // 1. Go to root (redirects to /login because user is not logged in)
    await page.goto('/');
    await expect(page).toHaveURL(/.*\/login/);

    // 2. Try invalid password
    await page.fill('#username', 'admin');
    await page.fill('#password', 'WrongPassword123!');
    await page.click('#btn-submit-login');
    await expect(page.locator('#login-error')).toBeVisible();
    await expect(page.locator('#login-error')).toContainText('Invalid');

    // 3. Valid login
    await page.fill('#password', 'AdminSecretPassword123!');
    await page.click('#btn-submit-login');

    await expect(page).toHaveURL('http://127.0.0.1:18999/');
    await expect(page.locator('#current-username')).toContainText('admin');
    await expect(page.locator('#current-user-role')).toContainText('admin');

    // 4. Session persistence after reload
    await page.reload();
    await expect(page.locator('#current-username')).toContainText('admin');

    // 5. Logout
    await page.click('#btn-logout');
    await expect(page).toHaveURL(/.*\/login/);

    // 6. Direct access to dashboard should redirect to login
    await page.goto('/');
    await expect(page).toHaveURL(/.*\/login/);
  });
});

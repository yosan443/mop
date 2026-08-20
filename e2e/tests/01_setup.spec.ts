import { test, expect } from '@playwright/test';

test.describe('Initial Setup Flow', () => {
  test('redirects to /setup and creates initial admin account', async ({ page }) => {
    // 1. Access root, should redirect to /setup
    await page.goto('/');
    await expect(page).toHaveURL(/.*\/setup/);
    await expect(page.locator('h1')).toContainText('初期セットアップ');

    // 2. Try short password
    await page.fill('#username', 'admin');
    await page.fill('#password', 'short');
    await page.fill('#confirmPassword', 'short');
    await page.click('#btn-submit-setup');
    await expect(page.locator('#setup-error')).toBeVisible();
    await expect(page.locator('#setup-error')).toContainText('10 文字以上');

    // 3. Password mismatch
    await page.fill('#password', 'ValidPassword123!');
    await page.fill('#confirmPassword', 'DifferentPassword123!');
    await page.click('#btn-submit-setup');
    await expect(page.locator('#setup-error')).toBeVisible();
    await expect(page.locator('#setup-error')).toContainText('一致しません');

    // 4. Valid submission
    await page.fill('#password', 'AdminSecretPassword123!');
    await page.fill('#confirmPassword', 'AdminSecretPassword123!');
    await page.click('#btn-submit-setup');

    // Should redirect to dashboard
    await expect(page).toHaveURL('http://127.0.0.1:18999/');
    await expect(page.locator('#current-username')).toContainText('admin');
    await expect(page.locator('#current-user-role')).toContainText('admin');

    // 5. Subsequent access to /setup should redirect to /
    await page.goto('/setup');
    await expect(page).toHaveURL('http://127.0.0.1:18999/');
  });
});

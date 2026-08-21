import { test, expect } from '@playwright/test';

test.use({ extraHTTPHeaders: { 'x-forwarded-for': '10.0.0.3' } });

test.describe('User Management (Admin)', () => {
  test('creates, modifies and disables user accounts', async ({ page }) => {
    // 1. Log in as admin
    await page.goto('/login');
    await page.waitForLoadState('networkidle');

    if (page.url().includes('/setup')) {
      await page.fill('#username', 'admin');
      await page.fill('#password', 'AdminSecretPassword123!');
      await page.fill('#confirmPassword', 'AdminSecretPassword123!');
      await page.click('#btn-submit-setup');
      await page.waitForURL('http://127.0.0.1:18999/');
    } else {
      await page.fill('#username', 'admin');
      await page.fill('#password', 'AdminSecretPassword123!');
      await page.click('#btn-submit-login');
      await page.waitForURL('http://127.0.0.1:18999/');
    }

    // 2. Navigate to user management
    await page.click('#nav-users');
    await page.waitForURL('http://127.0.0.1:18999/settings/users');

    // 3. Open create user modal
    await page.click('#btn-open-create-user');
    await expect(page.locator('#new-username')).toBeVisible();

    // 4. Create an operator user
    await page.fill('#new-username', 'operator1');
    await page.fill('#new-password', 'OperatorPass123!');
    await page.selectOption('#new-role', 'operator');
    await page.click('#btn-submit-new-user');

    // 5. Verify new user appears in list
    await expect(page.locator('#users-table')).toContainText('operator1');
  });
});

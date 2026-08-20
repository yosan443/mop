import { test, expect } from '@playwright/test';

test.describe('User Management Flow', () => {
  test('creates, modifies and disables user accounts', async ({ page }) => {
    // 1. Log in as admin
    await page.goto('/login');
    await page.fill('#username', 'admin');
    await page.fill('#password', 'AdminSecretPassword123!');
    await page.click('#btn-submit-login');
    await expect(page).toHaveURL('http://127.0.0.1:18999/');

    // 2. Click User Management in Navbar
    await page.click('#nav-users');
    await expect(page).toHaveURL(/.*\/settings\/users/);
    await expect(page.locator('h1')).toContainText('ユーザー管理');

    // 3. Open Create User modal
    await page.click('#btn-open-create-user');
    await page.fill('#new-username', 'operator_test');
    await page.fill('#new-password', 'OperatorPass123!');
    await page.selectOption('#new-role', 'operator');
    await page.click('#btn-submit-new-user');

    // 4. Verify user created in table
    const row = page.locator('#user-row-operator_test');
    await expect(row).toBeVisible();
    await expect(row).toContainText('operator_test');

    // 5. Change role to Viewer
    const roleSelect = row.locator('.role-select');
    await roleSelect.selectOption('viewer');
    await expect(page.locator('.alert-success')).toBeVisible();

    // 6. Toggle disable
    const disableBtn = row.locator('button');
    await disableBtn.click();
    await expect(row.locator('.badge-danger')).toContainText('無効');
  });
});

import { test, expect } from '@playwright/test';

test.use({ extraHTTPHeaders: { 'x-forwarded-for': '10.0.8.88' } });

test.describe('Viewer RBAC & Hidden Actions (M2)', () => {
  test('hides action buttons for viewer role', async ({ page }) => {
    // 1. Admin login & create viewer user
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

    // Navigate to users
    await page.click('#nav-users');
    await page.waitForURL((url) => url.pathname.includes('/settings/users'));

    // Create viewer and wait for user creation API to succeed (201 Created)
    await page.click('#btn-open-create-user');
    await page.fill('#new-username', 'viewer_bob');
    await page.fill('#new-password', 'ViewerBobPass123!');
    await page.selectOption('#new-role', 'viewer');

    const [createUserResponse] = await Promise.all([
      page.waitForResponse((res) => res.url().includes('/api/v1/users') && res.request().method() === 'POST'),
      page.click('#btn-submit-new-user'),
    ]);
    expect(createUserResponse.status()).toBe(201);
    await page.waitForLoadState('networkidle');

    // 2. Return to dashboard and logout admin
    await page.goto('/');
    await page.waitForLoadState('networkidle');
    await page.click('#btn-logout');
    await page.waitForURL((url) => url.pathname.includes('/login'));

    // 3. Login as viewer with explicit response wait
    await page.fill('#username', 'viewer_bob');
    await page.fill('#password', 'ViewerBobPass123!');

    const [loginResponse] = await Promise.all([
      page.waitForResponse((res) => res.url().includes('/api/v1/auth/login')),
      page.click('#btn-submit-login'),
    ]);

    expect(loginResponse.status()).toBe(200);
    await page.waitForURL((url) => url.pathname === '/');

    // 4. Verify action buttons do NOT exist on dashboard
    await expect(page.locator('.card-actions-footer')).not.toBeVisible();
    await expect(page.locator('#btn-restart-systemd-caddy-service')).not.toBeVisible();

    // 5. Navigate to resource detail
    await page.goto('/resources/systemd%3Acaddy.service');
    await page.waitForLoadState('networkidle');

    // 6. Verify action buttons do NOT exist on detail page
    await expect(page.locator('.action-buttons-group')).not.toBeVisible();
  });
});

import { test, expect } from '@playwright/test';
import path from 'path';

const ARTIFACT_DIR = '/home/yosan443/.gemini/antigravity-ide/brain/1b2cf549-9f57-4ef1-8a26-9ecf25e5523b';

test.describe('Visual Verification & Screenshot Capture', () => {
  test('captures key UI views for walkthrough', async ({ page }) => {
    // 1. Setup view (if not yet setup, or fresh)
    await page.goto('/');
    await page.waitForLoadState('networkidle');
    await page.screenshot({ path: path.join(ARTIFACT_DIR, '01_setup_view.png'), fullPage: true });

    // 2. Perform setup if on /setup
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

    await page.waitForLoadState('networkidle');
    // 3. Capture Dashboard view
    await page.screenshot({ path: path.join(ARTIFACT_DIR, '02_dashboard_view.png'), fullPage: true });

    // 4. Capture User Management view
    await page.click('#nav-users');
    await page.waitForURL(/.*\/settings\/users/);
    await page.waitForLoadState('networkidle');
    await page.screenshot({ path: path.join(ARTIFACT_DIR, '03_users_view.png'), fullPage: true });

    // 5. Open Create User modal
    await page.click('#btn-open-create-user');
    await page.waitForTimeout(200);
    await page.screenshot({ path: path.join(ARTIFACT_DIR, '04_create_user_modal.png'), fullPage: true });

    // 6. Create an operator user
    await page.fill('#new-username', 'operator_alice');
    await page.fill('#new-password', 'OperatorPass123!');
    await page.selectOption('#new-role', 'operator');
    await page.click('#btn-submit-new-user');
    await page.waitForTimeout(500);
    await page.screenshot({ path: path.join(ARTIFACT_DIR, '05_users_with_operator.png'), fullPage: true });
  });
});

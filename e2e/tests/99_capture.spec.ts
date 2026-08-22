import { test } from '@playwright/test';
import path from 'path';
import fs from 'fs';
import { fileURLToPath } from 'url';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

const LOCAL_ARTIFACT_DIR = '/home/yosan443/.gemini/antigravity-ide/brain/1b2cf549-9f57-4ef1-8a26-9ecf25e5523b';
const FALLBACK_DIR = path.resolve(__dirname, '../screenshots');

const TARGET_DIR = fs.existsSync(LOCAL_ARTIFACT_DIR) ? LOCAL_ARTIFACT_DIR : FALLBACK_DIR;
if (!fs.existsSync(TARGET_DIR)) {
  fs.mkdirSync(TARGET_DIR, { recursive: true });
}

test.use({ extraHTTPHeaders: { 'x-forwarded-for': '10.0.0.99' } });

test.describe('Visual Verification & Screenshot Capture (M3)', () => {
  test('captures key UI views for walkthrough', async ({ page }) => {
    // 1. Setup view (if not yet setup, or fresh)
    await page.goto('/');
    await page.waitForLoadState('networkidle');

    // 2. Perform setup if on /setup
    if (page.url().includes('/setup')) {
      await page.screenshot({ path: path.join(TARGET_DIR, '01_setup_view.png'), fullPage: true });
      await page.fill('#username', 'admin');
      await page.fill('#password', 'AdminSecretPassword123!');
      await page.fill('#confirmPassword', 'AdminSecretPassword123!');
      await page.click('#btn-submit-setup');
      await page.waitForURL((url) => url.pathname === '/');
    } else if (page.url().includes('/login')) {
      await page.screenshot({ path: path.join(TARGET_DIR, '01_setup_view.png'), fullPage: true });
      await page.fill('#username', 'admin');
      await page.fill('#password', 'AdminSecretPassword123!');
      await page.click('#btn-submit-login');
      await page.waitForURL((url) => url.pathname === '/');
    }

    await page.waitForLoadState('networkidle');
    // 3. Capture Dashboard view with Compose Projects Hierarchy & Unmanaged Badges
    await page.screenshot({ path: path.join(TARGET_DIR, '02_dashboard_view.png'), fullPage: true });

    // 4. Open Confirm Action Modal on Compose Project (shows managed & unmanaged containers)
    await page.click('#btn-restart-compose_project-media-stack');
    await page.locator('.modal-content').waitFor({ state: 'visible' });
    await page.locator('#compose-scope-box').waitFor({ state: 'visible' });
    await page.screenshot({ path: path.join(TARGET_DIR, '03_confirm_action_modal.png'), fullPage: true });
    await page.click('#btn-cancel-action');
    await page.locator('.modal-content').waitFor({ state: 'hidden' });

    // 5. Navigate to Compose Service Detail View (with depends_on, containers, and live logs)
    await page.click('#btn-detail-compose_service-media-stack-manga-worker');
    await page.waitForURL((url) => url.pathname.includes('/resources/compose_service:media-stack:manga-worker') || url.pathname.includes('/resources/compose_service%3Amedia-stack%3Amanga-worker'));
    await page.waitForLoadState('networkidle');
    await page.locator('#log-terminal-container').waitFor({ state: 'visible' });
    await page.screenshot({ path: path.join(TARGET_DIR, '04_resource_detail_logs.png'), fullPage: true });

    // 6. Capture User Management view
    await page.goto('/settings/users');
    await page.waitForLoadState('networkidle');
    await page.screenshot({ path: path.join(TARGET_DIR, '05_users_view.png'), fullPage: true });
  });
});

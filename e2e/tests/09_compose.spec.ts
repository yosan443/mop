import { test, expect } from '@playwright/test';

test.use({ extraHTTPHeaders: { 'x-forwarded-for': '10.0.0.99' } });

test.describe('Docker Compose Project, Services, and Protection E2E (M3)', () => {
  test('verifies Compose hierarchy, restart transitions, unmanaged protection, container scopes, and aggregated logs', async ({ page }) => {
    // 1. Initial login as admin
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

    // 2. Compose プロジェクト media-stack とサービスの階層表示
    const composeSection = page.locator('#section-compose-projects');
    await expect(composeSection).toBeVisible();
    await expect(composeSection).toContainText('Docker Compose Projects');

    const projectCard = page.locator('#resource-card-compose_project-media-stack');
    await expect(projectCard).toBeVisible();
    await expect(projectCard).toContainText('media-stack');
    await expect(projectCard).toContainText('1/2 managed');

    const mangaWorkerCard = page.locator('#resource-card-compose_service-media-stack-manga-worker');
    await expect(mangaWorkerCard).toBeVisible();
    await expect(mangaWorkerCard).toContainText('manga-worker');
    await expect(mangaWorkerCard).toContainText('depends_on:');
    await expect(mangaWorkerCard).toContainText('db');

    const dbCard = page.locator('#resource-card-compose_service-media-stack-db');
    await expect(dbCard).toBeVisible();
    await expect(dbCard).toContainText('db');

    // 3. 未管理サービス (db) に操作ボタンがないこと & 未管理バッジが表示されること
    await expect(dbCard.locator('#badge-unmanaged-compose_service-media-stack-db')).toBeVisible();
    await expect(dbCard.locator('#btn-restart-compose_service-media-stack-db')).not.toBeVisible();
    await expect(dbCard.locator('#btn-start-compose_service-media-stack-db')).not.toBeVisible();
    await expect(dbCard.locator('#btn-stop-compose_service-media-stack-db')).not.toBeVisible();

    // 4. 管理対象サービス (manga-worker) の再起動で restarting → running 遷移
    await page.click('#btn-restart-compose_service-media-stack-manga-worker');
    const serviceModal = page.locator('.modal-content');
    await expect(serviceModal).toBeVisible();
    await expect(serviceModal).toContainText('compose_service:media-stack:manga-worker');
    await page.click('#btn-confirm-action');
    await expect(serviceModal).not.toBeVisible();

    await expect(page.locator('.alert-success')).toBeVisible();
    await expect(page.locator('.alert-success')).toContainText('ジョブを受け付けました');

    // 5. プロジェクト再起動のモーダルで manga-worker のみが対象表示され、再起動後も db の状態が変わらないこと
    await page.click('#btn-restart-compose_project-media-stack');
    const projectModal = page.locator('.modal-content');
    await expect(projectModal).toBeVisible();
    await expect(projectModal).toContainText('compose_project:media-stack');

    const scopeBox = projectModal.locator('#compose-scope-box');
    await expect(scopeBox).toBeVisible();
    await expect(scopeBox.locator('#scope-managed-containers')).toContainText('media-stack-manga-worker-1');
    await expect(scopeBox.locator('#scope-unmanaged-containers')).toContainText('media-stack-db-1');
    await expect(scopeBox.locator('#scope-unmanaged-containers')).toContainText('保護');

    await page.click('#btn-confirm-action');
    await expect(projectModal).not.toBeVisible();
    await expect(page.locator('.alert-success')).toBeVisible();

    // db サービスの状態が running のままであることを確認
    await expect(dbCard).toContainText('running');
    await expect(dbCard.locator('.badge-success')).toBeVisible();

    // 6. compose_service 詳細で depends_on と統合ログ ([service|container] プレフィックス付き) が表示されること
    await page.click('#btn-detail-compose_service-media-stack-manga-worker');
    await page.waitForURL((url) =>
      url.pathname.includes('/resources/compose_service:media-stack:manga-worker') ||
      url.pathname.includes('/resources/compose_service%3Amedia-stack%3Amanga-worker')
    );

    // depends_on と構成コンテナの確認
    await expect(page.locator('#detail-resource-name')).toContainText('manga-worker');
    await expect(page.locator('#row-depends-on')).toContainText('db');
    await expect(page.locator('#block-containers-list')).toContainText('media-stack-manga-worker-1');
    await expect(page.locator('#badge-managed-status')).toBeVisible();

    // 統合ログ ([service|container] プレフィックス付き) の確認
    const logTerminal = page.locator('#log-terminal-container');
    await expect(logTerminal).toBeVisible();
    await expect(logTerminal.locator('.log-row').first()).toBeVisible({ timeout: 10000 });
    await expect(logTerminal).toContainText('[manga-worker|media-stack-manga-worker-1]');

    // Status is running
    const statusBadge = page.locator('#detail-status-badge');
    await expect(statusBadge).toHaveText(/running|restarting/);
    await expect(statusBadge).toHaveText('running', { timeout: 10000 });
  });
});

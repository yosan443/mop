import { test, expect } from '@playwright/test';

test.use({ extraHTTPHeaders: { 'x-forwarded-for': '10.0.0.10' } });

test.describe('M4 Full-stack Plugin E2E Lifecycle', () => {
  test('scans, enables, executes jobs, and updates settings on real mop.hello plugin', async ({ page }) => {
    page.on('console', (msg) => console.log(`[Browser Console] ${msg.type()}: ${msg.text()}`));
    page.on('response', async (res) => {
      if (res.status() >= 400) {
        let text = '';
        try { text = await res.text(); } catch {}
        console.log(`[HTTP Error] ${res.status()} ${res.url()}: ${text}`);
      }
    });

    // 1. Log in or complete initial setup
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

    // 2. Navigate to Plugins View
    await page.click('#nav-plugins');
    await page.waitForURL('http://127.0.0.1:18999/plugins');

    // Trigger scan / refresh to discover mop.hello
    await page.click('#btn-refresh-plugins');
    const pluginCard = page.locator('#plugin-card-mop-hello');
    await expect(pluginCard).toBeVisible({ timeout: 10000 });
    await expect(pluginCard).toContainText('Hello Plugin');
    await expect(pluginCard).toContainText('v0.1.0');

    // 3. Capability consent & enablement
    const enableBtn = page.locator('#btn-enable-mop-hello');
    await expect(enableBtn).toBeVisible();
    await enableBtn.click();

    // Verify capability consent modal opens
    const consentModal = page.locator('.modal-card');
    await expect(consentModal).toBeVisible();
    await expect(consentModal).toContainText('プラグインの有効化と権限承認');
    await expect(consentModal).toContainText('hello.ping');

    // Confirm enablement
    await page.click('#btn-confirm-enable');
    await expect(consentModal).not.toBeVisible();

    // Verify plugin is now running
    await expect(pluginCard.locator('.badge-success')).toBeVisible({ timeout: 10000 });
    await expect(pluginCard.locator('.badge-success')).toContainText('RUNNING');

    // 4. Test Condition 1: job.submit with non-granted job_type is rejected with 403 CAPABILITY_REQUIRED
    const unauthJobRes = await page.request.post('/api/v1/plugins/mop.hello/rpc', {
      data: {
        jsonrpc: '2.0',
        method: 'job.submit',
        params: { job_type: 'unauthorized.job' },
        id: 99,
      },
      headers: {
        origin: 'http://127.0.0.1:18999',
      },
    });
    expect(unauthJobRes.status()).toBe(403);
    const unauthBody = await unauthJobRes.json();
    expect(unauthBody.error.message).toContain('CAPABILITY_REQUIRED');

    // 5. Open Plugin Custom Element UI
    const openUiBtn = page.locator('#btn-open-ui-mop-hello');
    await expect(openUiBtn).toBeVisible();
    await openUiBtn.click();
    await page.waitForURL('http://127.0.0.1:18999/plugins/mop.hello');

    // Verify Custom Element is loaded and rendered inside shadow root
    const customEl = page.locator('mop-plugin-hello');
    await expect(customEl).toBeVisible({ timeout: 10000 });
    await expect(customEl).toContainText('First-party Custom Element Plugin running inside sandbox');

    // Submit ping job via Custom Element button
    const pingBtn = customEl.locator('#btn-ping');
    await expect(pingBtn).toBeVisible();
    await pingBtn.click();

    // Verify Custom Element output confirms submission
    const outputBox = customEl.locator('#output');
    await expect(outputBox).toContainText('Job submitted successfully:', { timeout: 10000 });

    // 6. Navigate to Jobs View and verify job completion & DB-persisted events
    await page.click('#nav-jobs');
    await page.waitForURL('http://127.0.0.1:18999/jobs');

    const jobCard = page.locator('.job-card').first();
    await expect(jobCard).toBeVisible({ timeout: 10000 });
    await expect(jobCard.locator('.job-kind')).toContainText('hello.ping');

    // Wait until job reaches SUCCEEDED status (Condition 3: avoid live intermediate SSE timing, wait for final state)
    await expect(jobCard.locator('[data-test="job-status"]')).toContainText('SUCCEEDED', {
      timeout: 10000,
    });

    // Expand job card to inspect persisted event logs and progress
    await jobCard.click();
    const eventLogs = page.locator('[data-test="job-event-msg"]');
    await expect(eventLogs.filter({ hasText: 'Greeting configured: Hello from mop plugin!' })).toBeVisible({
      timeout: 5000,
    });
    const progressBadges = page.locator('[data-test="job-progress"]');
    await expect(progressBadges.filter({ hasText: '75%' })).toBeVisible();

    // 7. Update Plugin Settings (Edit -> Save Draft -> Diff preview -> Apply)
    await page.click('#btn-back-dashboard');
    await page.waitForURL('http://127.0.0.1:18999/');

    await page.click('#nav-plugins');
    await page.waitForURL('http://127.0.0.1:18999/plugins');

    const settingsBtn = page.locator('#btn-settings-mop-hello');
    await expect(settingsBtn).toBeVisible();
    await settingsBtn.click();

    // Modal opens on Edit tab
    const settingsInput = page.locator('#settings-json-input');
    await expect(settingsInput).toBeVisible();
    await settingsInput.fill(JSON.stringify({ greeting: 'Konnichiwa E2E!' }, null, 2));

    // Save draft
    await page.click('#btn-save-draft');
    await expect(page.locator('.banner-success')).toContainText('下書き設定を保存しました');

    // Switch to Diff tab
    await page.click('#tab-settings-diff');
    await expect(page.locator('#settings-diff-list')).toBeVisible();

    // Verify applied_value and draft_value in diff
    const appliedVal = page.locator('[data-test="diff-applied"]');
    const draftVal = page.locator('[data-test="diff-draft"]');
    await expect(appliedVal).toBeVisible();
    await expect(draftVal).toBeVisible();
    await expect(draftVal).toContainText('Konnichiwa E2E!');

    // Apply settings (this runs config.validate, promotes draft, config.apply, and restarts plugin)
    await page.click('#btn-apply-settings');
    await expect(page.locator('#settings-diff-list')).not.toBeVisible({ timeout: 10000 });

    // Wait for plugin process restart to complete
    await expect(pluginCard.locator('.badge-success')).toContainText('RUNNING', { timeout: 10000 });

    // 8. Execute another job and verify that the new greeting was applied in the restarted plugin
    await page.click('#btn-open-ui-mop-hello');
    await page.waitForURL('http://127.0.0.1:18999/plugins/mop.hello');

    const newPingBtn = page.locator('mop-plugin-hello #btn-ping');
    await expect(newPingBtn).toBeVisible({ timeout: 10000 });
    await newPingBtn.click();

    await expect(page.locator('mop-plugin-hello #output')).toContainText('Job submitted successfully:', {
      timeout: 10000,
    });

    // Check Jobs View for the second job
    await page.click('#nav-jobs');
    await page.waitForURL('http://127.0.0.1:18999/jobs');

    const newestJobCard = page.locator('.job-card').first();
    await expect(newestJobCard.locator('[data-test="job-status"]')).toContainText('SUCCEEDED', {
      timeout: 10000,
    });

    // Expand newest job
    await newestJobCard.click();
    const newEventLogs = page.locator('[data-test="job-event-msg"]');
    await expect(
      newEventLogs.filter({ hasText: 'Greeting configured: Konnichiwa E2E!' })
    ).toBeVisible({ timeout: 5000 });
  });
});

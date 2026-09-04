import { test, expect } from '@playwright/test';

test.use({ extraHTTPHeaders: { 'x-forwarded-for': '10.0.0.10' } });

test.describe('Plugin Management & Settings Diff Rendering', () => {
  test('renders applied_value and draft_value in diff tab and applies settings', async ({ page }) => {
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

    let isApplied = false;

    // 2. Intercept Plugin API endpoints to test frontend rendering of diff items
    await page.route('**/api/v1/plugins', (route) => {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify([
          {
            id: 'mop.hello',
            name: 'Hello Plugin',
            version: '0.1.0',
            api_version: '1',
            enabled: true,
            state: 'running',
            installed_at: new Date().toISOString(),
            updated_at: new Date().toISOString(),
            permissions: [
              {
                plugin_id: 'mop.hello',
                capability: 'jobs',
                value_json: '["hello.ping"]',
                granted_by: 'admin',
                granted_at: new Date().toISOString(),
              },
            ],
            applied_settings: { greeting: isApplied ? 'New Awesome Greeting' : 'Old Greeting' },
          },
        ]),
      });
    });

    await page.route('**/api/v1/plugins/mop.hello/settings', (route, request) => {
      if (request.method() === 'GET') {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({
            plugin_id: 'mop.hello',
            applied: { greeting: isApplied ? 'New Awesome Greeting' : 'Old Greeting' },
            draft: isApplied ? {} : { greeting: 'New Awesome Greeting' },
            diff: {
              plugin_id: 'mop.hello',
              items: isApplied
                ? []
                : [
                    {
                      key: 'greeting',
                      applied_value: 'Old Greeting',
                      draft_value: 'New Awesome Greeting',
                      change_type: 'modified',
                    },
                  ],
            },
          }),
        });
      } else {
        route.continue();
      }
    });

    await page.route('**/api/v1/plugins/mop.hello/settings/diff', (route) => {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({
          plugin_id: 'mop.hello',
          items: isApplied
            ? []
            : [
                {
                  key: 'greeting',
                  applied_value: 'Old Greeting',
                  draft_value: 'New Awesome Greeting',
                  change_type: 'modified',
                },
              ],
        }),
      });
    });

    await page.route('**/api/v1/plugins/mop.hello/settings/apply', (route) => {
      isApplied = true;
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ status: 'applied', id: 'mop.hello' }),
      });
    });

    // 3. Navigate to Plugins view
    await page.click('#nav-plugins');
    await page.waitForURL('http://127.0.0.1:18999/plugins');

    // 4. Verify plugin item is listed
    await expect(page.locator('#plugin-card-mop-hello')).toBeVisible();

    // 5. Open Settings Modal
    await page.click('#btn-settings-mop-hello');
    await expect(page.locator('#tab-settings-diff')).toBeVisible();

    // 6. Switch to Diff Tab
    await page.click('#tab-settings-diff');
    await expect(page.locator('#settings-diff-list')).toBeVisible();

    // 7. Verify applied_value and draft_value are correctly rendered!
    const appliedValueEl = page.locator('[data-test="diff-applied"]');
    const draftValueEl = page.locator('[data-test="diff-draft"]');

    await expect(appliedValueEl).toBeVisible();
    await expect(appliedValueEl).toContainText('Old Greeting');

    await expect(draftValueEl).toBeVisible();
    await expect(draftValueEl).toContainText('New Awesome Greeting');

    // 8. Click Apply Settings
    await page.click('#btn-apply-settings');

    // 9. Wait for modal to close
    await expect(page.locator('#settings-diff-list')).not.toBeVisible();
  });
});

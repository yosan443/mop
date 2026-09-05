import { test, expect } from '@playwright/test';
import { execSync } from 'child_process';
import fs from 'fs';
import os from 'os';
import path from 'path';

test.use({ extraHTTPHeaders: { 'x-forwarded-for': '10.0.0.11' } });

function createSampleImage(filePath: string) {
  execSync(
    `ffmpeg -f lavfi -i color=c=blue:s=64x64 -frames:v 1 -update 1 "${filePath}" -y`,
    { stdio: 'ignore' }
  );
}

function createSampleMangaArchive(zipPath: string, pageCount: number = 6) {
  const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'manga-pages-'));
  try {
    for (let i = 0; i < pageCount; i++) {
      const pagePath = path.join(tmpDir, `page_${String(i).padStart(2, '0')}.jpg`);
      createSampleImage(pagePath);
    }
    execSync(
      `python3 -c "import zipfile, os; zf = zipfile.ZipFile('${zipPath}', 'w'); [zf.write(os.path.join('${tmpDir}', f), f) for f in sorted(os.listdir('${tmpDir}'))]; zf.close()"`,
      { stdio: 'ignore' }
    );
  } finally {
    fs.rmSync(tmpDir, { recursive: true, force: true });
  }
}

async function ensureLoggedIn(page: any) {
  await page.goto('/login');
  await page.waitForLoadState('networkidle');

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
}

test.describe('M5 Part 3: Manga Plugin Full Lifecycle & Watcher E2E', () => {
  const testBaseDir = fs.mkdtempSync(path.join(os.tmpdir(), 'mop-manga-test-'));
  const watchDir = path.join(testBaseDir, 'watch');
  const outputDir = path.join(testBaseDir, 'output');
  const unknownDir = path.join(testBaseDir, 'unknown');
  const workDir = path.join(testBaseDir, 'work');

  test.beforeAll(() => {
    fs.mkdirSync(watchDir, { recursive: true });
    fs.mkdirSync(outputDir, { recursive: true });
    fs.mkdirSync(unknownDir, { recursive: true });
    fs.mkdirSync(workDir, { recursive: true });
  });

  test.afterAll(() => {
    try {
      fs.rmSync(testBaseDir, { recursive: true, force: true });
    } catch {}
  });

  test('mop.manga: scan, enable, doctor, validate rejection, manual convert, and watcher conversion', async ({ page }) => {
    test.setTimeout(120000);

    // 1. Log in or complete initial setup
    await ensureLoggedIn(page);

    // 2. Navigate to Plugins View and scan for mop.manga
    await page.goto('/plugins');
    await page.waitForLoadState('networkidle');

    await page.click('#btn-refresh-plugins');
    const mangaCard = page.locator('#plugin-card-mop-manga');
    await expect(mangaCard).toBeVisible({ timeout: 10000 });
    await expect(mangaCard).toContainText('Manga Conversion');
    await expect(mangaCard).toContainText('v1.0.0');

    // 3. Capability consent & enablement
    const enableBtn = page.locator('#btn-enable-mop-manga');
    if (await enableBtn.isVisible()) {
      await enableBtn.click();
      const modal = page.locator('.modal-card');
      await expect(modal).toBeVisible();
      await expect(modal).toContainText('manga.convert');
      await expect(modal).toContainText('manga.batch');
      await expect(modal).toContainText('manga.inspect');

      await page.click('#btn-confirm-enable');
      await expect(modal).not.toBeVisible();
    }

    // Verify running status
    await expect(mangaCard.locator('.badge-success')).toContainText('RUNNING', { timeout: 10000 });

    // 4. Open Custom Element UI and run Doctor diagnosis
    await page.click('#btn-open-ui-mop-manga');
    await page.waitForURL('http://127.0.0.1:18999/plugins/mop.manga');

    const customEl = page.locator('mop-plugin-manga');
    await expect(customEl).toBeVisible({ timeout: 10000 });
    await expect(customEl).toContainText('Manga Plugin (mop.manga)');

    // Run Doctor via UI button
    const doctorBtn = customEl.locator('#btn-run-doctor-header');
    await expect(doctorBtn).toBeVisible();
    await doctorBtn.click();

    // Verify doctor results
    const doctorResults = customEl.locator('#doctor-results');
    await expect(doctorResults).toBeVisible({ timeout: 10000 });
    await expect(doctorResults).toContainText('ok');
    const outputBox = customEl.locator('#output');
    await expect(outputBox).toContainText('"status": "ok"', { timeout: 10000 });

    // 5. Settings Validation: Verify layout overlap rejection
    await page.goto('/plugins');
    await page.waitForLoadState('networkidle');

    const settingsBtn = page.locator('#btn-settings-mop-manga');
    await expect(settingsBtn).toBeVisible();
    await settingsBtn.click();

    // Fill invalid overlapping layout
    const invalidSettings = {
      watch_dirs: [path.join(testBaseDir, 'overlap')],
      output_dir: path.join(testBaseDir, 'overlap/nested_out'),
    };
    const settingsInput = page.locator('#settings-json-input');
    await expect(settingsInput).toBeVisible();
    await settingsInput.fill(JSON.stringify(invalidSettings, null, 2));

    await page.click('#btn-save-draft');
    await expect(page.locator('.banner-success')).toContainText('下書き設定を保存しました');

    await page.click('#tab-settings-diff');
    await expect(page.locator('#settings-diff-list')).toBeVisible();

    // Apply settings should fail with validation error
    await page.click('#btn-apply-settings');
    const errorBanner = page.locator('.banner-error');
    await expect(errorBanner).toBeVisible({ timeout: 10000 });
    await expect(errorBanner).toContainText('overlap');

    // Close modal
    await page.click('.btn-close');
    await expect(page.locator('.modal-card')).not.toBeVisible();

    // 6. Apply valid settings with active watcher
    await settingsBtn.click();
    await page.click('#tab-settings-edit');
    const validSettings = {
      watch_dirs: [watchDir],
      output_dir: outputDir,
      unknown_dir: unknownDir,
      work_dir: workDir,
      workers: 1,
      series_subdir: false,
      delete_original: false,
      overwrite: true,
      scan_on_start: false,
    };
    await settingsInput.fill(JSON.stringify(validSettings, null, 2));
    await page.click('#btn-save-draft');
    await page.click('#tab-settings-diff');
    await page.click('#btn-apply-settings');
    await expect(page.locator('.modal-card')).not.toBeVisible({ timeout: 10000 });

    // Verify plugin restarted cleanly
    await expect(mangaCard.locator('.badge-success')).toContainText('RUNNING', { timeout: 10000 });

    // 7. Manual conversion of a real ZIP archive via UI
    const manualZipPath = path.join(testBaseDir, 'manual_volume_01.zip');
    createSampleMangaArchive(manualZipPath, 6);
    expect(fs.existsSync(manualZipPath)).toBe(true);

    await page.click('#btn-open-ui-mop-manga');
    await page.waitForURL('http://127.0.0.1:18999/plugins/mop.manga');

    const convertInput = customEl.locator('#convert-input');
    await expect(convertInput).toBeVisible();
    await convertInput.fill(manualZipPath);

    const submitConvertBtn = customEl.locator('#btn-convert-submit');
    await submitConvertBtn.click();
    await expect(customEl.locator('#output')).toContainText('ジョブ送信完了:', { timeout: 10000 });

    // Verify job in Jobs view reaches SUCCEEDED
    await page.goto('/jobs');
    await page.waitForLoadState('networkidle');

    const jobCard = page.locator('.job-card').first();
    await expect(jobCard).toBeVisible({ timeout: 10000 });
    await expect(jobCard.locator('.job-kind')).toContainText('manga.convert');
    await expect(jobCard.locator('[data-test="job-status"]')).toContainText('SUCCEEDED', { timeout: 20000 });

    // Helper to find any generated CBZ in directory tree
    const getCbzFiles = (dir: string) => {
      const found: string[] = [];
      const scan = (d: string) => {
        if (!fs.existsSync(d)) return;
        for (const f of fs.readdirSync(d, { withFileTypes: true })) {
          const full = path.join(d, f.name);
          if (f.isDirectory()) scan(full);
          else if (f.isFile() && f.name.endsWith('.cbz')) found.push(full);
        }
      };
      scan(dir);
      return found;
    };

    // Verify CBZ output exists in outputDir
    await expect.poll(() => getCbzFiles(outputDir).length, { timeout: 15000 }).toBeGreaterThan(0);
    const cbzFiles = getCbzFiles(outputDir);
    expect(fs.statSync(cbzFiles[0]).size).toBeGreaterThan(0);

    // 8. Watcher automatic conversion
    const watcherZipPath = path.join(watchDir, 'auto_volume_02.zip');
    createSampleMangaArchive(watcherZipPath, 6);
    expect(fs.existsSync(watcherZipPath)).toBe(true);

    // Watcher has a 2-second debounce, then processes to outputDir
    await expect.poll(() => getCbzFiles(outputDir).length, { timeout: 20000 }).toBeGreaterThan(1);
    const updatedCbzFiles = getCbzFiles(outputDir);
    expect(updatedCbzFiles.length).toBeGreaterThanOrEqual(2);
    for (const f of updatedCbzFiles) {
      expect(fs.statSync(f).size).toBeGreaterThan(0);
    }

    // 9. Batch conversion via UI (target directory with 1 file -> UI batch submit -> SUCCEEDED)
    const batchIncomingDir = path.join(testBaseDir, 'batch_incoming');
    fs.mkdirSync(batchIncomingDir, { recursive: true });
    const batchZipPath = path.join(batchIncomingDir, 'batch_volume_03.zip');
    createSampleMangaArchive(batchZipPath, 6);
    expect(fs.existsSync(batchZipPath)).toBe(true);

    await page.goto('/plugins/mop.manga');
    await page.waitForLoadState('networkidle');

    // Switch to Batch tab in Custom Element UI
    const batchTabBtn = customEl.locator('#tab-batch');
    await expect(batchTabBtn).toBeVisible({ timeout: 10000 });
    await batchTabBtn.click();

    const batchInput = customEl.locator('#batch-input');
    await expect(batchInput).toBeVisible();
    await batchInput.fill(batchIncomingDir);

    const submitBatchBtn = customEl.locator('#btn-batch-submit');
    await submitBatchBtn.click();
    await expect(customEl.locator('#output')).toContainText('ジョブ送信完了:', { timeout: 10000 });

    // Verify batch job in Jobs view reaches SUCCEEDED
    await page.goto('/jobs');
    await page.waitForLoadState('networkidle');

    const batchJobCard = page.locator('.job-card').filter({ hasText: 'manga.batch' }).first();
    await expect(batchJobCard).toBeVisible({ timeout: 10000 });
    await expect(batchJobCard.locator('[data-test="job-status"]')).toContainText('SUCCEEDED', { timeout: 25000 });

    // Verify CBZ from batch exists in outputDir
    await expect.poll(() => getCbzFiles(outputDir).length, { timeout: 20000 }).toBeGreaterThan(2);
    const finalCbzFiles = getCbzFiles(outputDir);
    expect(finalCbzFiles.length).toBeGreaterThanOrEqual(3);
    for (const f of finalCbzFiles) {
      expect(fs.statSync(f).size).toBeGreaterThan(0);
    }
  });
});

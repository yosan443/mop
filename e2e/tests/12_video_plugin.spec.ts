import { test, expect } from '@playwright/test';
import { execSync } from 'child_process';
import fs from 'fs';
import os from 'os';
import path from 'path';

test.use({ extraHTTPHeaders: { 'x-forwarded-for': '10.0.0.12' } });

function createSampleVideo(filePath: string, durationSec: number = 1) {
  execSync(
    `ffmpeg -f lavfi -i testsrc=duration=${durationSec}:size=160x120:rate=10 -c:v libx264 "${filePath}" -y`,
    { stdio: 'ignore' }
  );
}

function createSampleMangaArchive(zipPath: string, pageCount: number = 6) {
  const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'coexist-manga-pages-'));
  try {
    for (let i = 0; i < pageCount; i++) {
      const pagePath = path.join(tmpDir, `page_${String(i).padStart(2, '0')}.jpg`);
      execSync(
        `ffmpeg -f lavfi -i color=c=blue:s=64x64 -frames:v 1 -update 1 "${pagePath}" -y`,
        { stdio: 'ignore' }
      );
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

test.describe('M5 Part 3: Video Plugin Full Lifecycle & Coexistence E2E', () => {
  const testBaseDir = fs.mkdtempSync(path.join(os.tmpdir(), 'mop-video-test-'));
  const videoWatchDir = path.join(testBaseDir, 'video_watch');
  const videoOutputDir = path.join(testBaseDir, 'video_output');
  const videoWorkDir = path.join(testBaseDir, 'video_work');

  const sharedWatchDir = path.join(testBaseDir, 'shared_watch');
  const mangaCoexistOutDir = path.join(testBaseDir, 'manga_coexist_out');
  const mangaCoexistWorkDir = path.join(testBaseDir, 'manga_coexist_work');
  const videoCoexistOutDir = path.join(testBaseDir, 'video_coexist_out');
  const videoCoexistWorkDir = path.join(testBaseDir, 'video_coexist_work');

  test.beforeAll(() => {
    fs.mkdirSync(videoWatchDir, { recursive: true });
    fs.mkdirSync(videoOutputDir, { recursive: true });
    fs.mkdirSync(videoWorkDir, { recursive: true });

    fs.mkdirSync(sharedWatchDir, { recursive: true });
    fs.mkdirSync(mangaCoexistOutDir, { recursive: true });
    fs.mkdirSync(mangaCoexistWorkDir, { recursive: true });
    fs.mkdirSync(videoCoexistOutDir, { recursive: true });
    fs.mkdirSync(videoCoexistWorkDir, { recursive: true });
  });

  test.afterAll(() => {
    try {
      fs.rmSync(testBaseDir, { recursive: true, force: true });
    } catch {}
  });

  test('mop.video: scan, enable, doctor, validate rejection, transcode, and watcher conversion', async ({ page }) => {
    test.setTimeout(90000);

    // 1. Log in or complete initial setup
    await ensureLoggedIn(page);

    // 2. Navigate to Plugins View and scan for mop.video
    await page.goto('/plugins');
    await page.waitForLoadState('networkidle');

    await page.click('#btn-refresh-plugins');
    const videoCard = page.locator('#plugin-card-mop-video');
    await expect(videoCard).toBeVisible({ timeout: 10000 });
    await expect(videoCard).toContainText('Video Transcoding');
    await expect(videoCard).toContainText('v0.1.0');

    // 3. Capability consent & enablement
    const enableBtn = page.locator('#btn-enable-mop-video');
    if (await enableBtn.isVisible()) {
      await enableBtn.click();
      const modal = page.locator('.modal-card');
      await expect(modal).toBeVisible();
      await expect(modal).toContainText('video.convert');
      await expect(modal).toContainText('video.batch');
      await expect(modal).toContainText('video.inspect');

      await page.click('#btn-confirm-enable');
      await expect(modal).not.toBeVisible();
    }

    // Verify running status
    await expect(videoCard.locator('.badge-success')).toContainText('RUNNING', { timeout: 10000 });

    // 4. Open Custom Element UI and run Doctor diagnosis (ffmpeg & libx265)
    await page.click('#btn-open-ui-mop-video');
    await page.waitForURL('http://127.0.0.1:18999/plugins/mop.video');

    const customEl = page.locator('mop-plugin-video');
    await expect(customEl).toBeVisible({ timeout: 10000 });
    await expect(customEl).toContainText('Video Plugin (mop.video)');

    const doctorBtn = customEl.locator('#btn-run-doctor-header');
    await expect(doctorBtn).toBeVisible();
    await doctorBtn.click();

    // Verify doctor results
    const doctorResults = customEl.locator('#doctor-results');
    await expect(doctorResults).toBeVisible({ timeout: 10000 });
    await expect(doctorResults).toContainText('ok');
    const outputBox = customEl.locator('#output');
    await expect(outputBox).toContainText('"status": "ok"', { timeout: 10000 });
    await expect(outputBox).toContainText('libx265');

    // 5. Settings Validation: Verify layout overlap rejection
    await page.goto('/plugins');
    await page.waitForLoadState('networkidle');

    const settingsBtn = page.locator('#btn-settings-mop-video');
    await expect(settingsBtn).toBeVisible();
    await settingsBtn.click();

    const invalidSettings = {
      watch_dirs: [path.join(testBaseDir, 'overlap')],
      video_dir: path.join(testBaseDir, 'overlap/nested_video'),
    };
    const settingsInput = page.locator('#settings-json-input');
    await expect(settingsInput).toBeVisible();
    await settingsInput.fill(JSON.stringify(invalidSettings, null, 2));

    await page.click('#btn-save-draft');
    await expect(page.locator('.banner-success')).toContainText('下書き設定を保存しました');

    await page.click('#tab-settings-diff');
    await expect(page.locator('#settings-diff-list')).toBeVisible();

    await page.click('#btn-apply-settings');
    const errorBanner = page.locator('.banner-error');
    await expect(errorBanner).toBeVisible({ timeout: 10000 });
    await expect(errorBanner).toContainText('overlap');

    await page.click('.btn-close');
    await expect(page.locator('.modal-card')).not.toBeVisible();

    // 6. Apply valid settings with active watcher
    await settingsBtn.click();
    await page.click('#tab-settings-edit');
    const validSettings = {
      watch_dirs: [videoWatchDir],
      video_dir: videoOutputDir,
      work_dir: videoWorkDir,
      workers: 1,
      preset: 'ultrafast',
      crf: 28,
      delete_original: false,
      overwrite: true,
      scan_on_start: false,
    };
    await settingsInput.fill(JSON.stringify(validSettings, null, 2));
    await page.click('#btn-save-draft');
    await page.click('#tab-settings-diff');
    await page.click('#btn-apply-settings');
    await expect(page.locator('.modal-card')).not.toBeVisible({ timeout: 10000 });

    await expect(videoCard.locator('.badge-success')).toContainText('RUNNING', { timeout: 10000 });

    // 7. Manual transcode of synthetic video via UI
    const manualVideoPath = path.join(testBaseDir, 'manual_clip_01.mkv');
    createSampleVideo(manualVideoPath, 1);
    expect(fs.existsSync(manualVideoPath)).toBe(true);

    await page.click('#btn-open-ui-mop-video');
    await page.waitForURL('http://127.0.0.1:18999/plugins/mop.video');

    const convertInput = customEl.locator('#convert-input');
    await expect(convertInput).toBeVisible();
    await convertInput.fill(manualVideoPath);

    const submitConvertBtn = customEl.locator('#btn-convert-submit');
    await submitConvertBtn.click();
    await expect(customEl.locator('#output')).toContainText('ジョブ送信完了:', { timeout: 10000 });

    // Verify job in Jobs view reaches SUCCEEDED
    await page.goto('/jobs');
    await page.waitForLoadState('networkidle');

    const jobCard = page.locator('.job-card').first();
    await expect(jobCard).toBeVisible({ timeout: 10000 });
    await expect(jobCard.locator('.job-kind')).toContainText('video.convert');
    await expect(jobCard.locator('[data-test="job-status"]')).toContainText('SUCCEEDED', { timeout: 25000 });

    // Verify MP4 output exists in videoOutputDir
    const expectedManualMp4 = path.join(videoOutputDir, 'manual_clip_01.mp4');
    await expect.poll(() => fs.existsSync(expectedManualMp4), { timeout: 10000 }).toBe(true);
    expect(fs.statSync(expectedManualMp4).size).toBeGreaterThan(0);

    // 8. Watcher automatic transcode
    const watcherVideoPath = path.join(videoWatchDir, 'auto_clip_02.mkv');
    createSampleVideo(watcherVideoPath, 1);
    expect(fs.existsSync(watcherVideoPath)).toBe(true);

    const expectedAutoMp4 = path.join(videoOutputDir, 'auto_clip_02.mp4');
    // Watcher has 2s debounce, then encodes to HEVC MP4
    await expect.poll(() => fs.existsSync(expectedAutoMp4), { timeout: 25000 }).toBe(true);
    expect(fs.statSync(expectedAutoMp4).size).toBeGreaterThan(0);
  });

  test('Coexistence: mop.manga and mop.video simultaneously watch the same directory without conflict', async ({ page }) => {
    test.setTimeout(90000);

    // 1. Ensure logged in and navigate to Plugins View
    await ensureLoggedIn(page);
    await page.goto('/plugins');
    await page.waitForLoadState('networkidle');

    await page.click('#btn-refresh-plugins');
    const mangaCard = page.locator('#plugin-card-mop-manga');
    await expect(mangaCard).toBeVisible({ timeout: 10000 });
    const enableMangaBtn = page.locator('#btn-enable-mop-manga');
    if (await enableMangaBtn.isVisible()) {
      await enableMangaBtn.click();
      await page.click('#btn-confirm-enable');
      await expect(mangaCard.locator('.badge-success')).toContainText('RUNNING', { timeout: 10000 });
    }

    const videoCard = page.locator('#plugin-card-mop-video');
    await expect(videoCard).toBeVisible({ timeout: 10000 });
    const enableVideoBtn = page.locator('#btn-enable-mop-video');
    if (await enableVideoBtn.isVisible()) {
      await enableVideoBtn.click();
      await page.click('#btn-confirm-enable');
      await expect(videoCard.locator('.badge-success')).toContainText('RUNNING', { timeout: 10000 });
    }

    // 2. Configure mop.manga to watch sharedWatchDir
    const mangaSettingsBtn = page.locator('#btn-settings-mop-manga');
    await expect(mangaSettingsBtn).toBeVisible({ timeout: 10000 });
    await mangaSettingsBtn.click();

    const mangaSettingsInput = page.locator('#settings-json-input');
    await expect(mangaSettingsInput).toBeVisible();
    await page.click('#tab-settings-edit');
    const mangaCoexistSettings = {
      watch_dirs: [sharedWatchDir],
      output_dir: mangaCoexistOutDir,
      unknown_dir: path.join(testBaseDir, 'manga_coexist_unknown'),
      work_dir: mangaCoexistWorkDir,
      workers: 1,
      series_subdir: false,
      delete_original: false,
      overwrite: true,
      scan_on_start: false,
    };
    await mangaSettingsInput.fill(JSON.stringify(mangaCoexistSettings, null, 2));
    await page.click('#btn-save-draft');
    await page.click('#tab-settings-diff');
    await page.click('#btn-apply-settings');
    await expect(page.locator('.modal-card')).not.toBeVisible({ timeout: 10000 });

    // 3. Configure mop.video to watch sharedWatchDir
    const videoSettingsBtn = page.locator('#btn-settings-mop-video');
    await expect(videoSettingsBtn).toBeVisible();
    await videoSettingsBtn.click();

    const videoSettingsInput = page.locator('#settings-json-input');
    await expect(videoSettingsInput).toBeVisible();
    await page.click('#tab-settings-edit');
    const videoCoexistSettings = {
      watch_dirs: [sharedWatchDir],
      video_dir: videoCoexistOutDir,
      work_dir: videoCoexistWorkDir,
      workers: 1,
      preset: 'ultrafast',
      crf: 28,
      delete_original: false,
      overwrite: true,
      scan_on_start: false,
    };
    await videoSettingsInput.fill(JSON.stringify(videoCoexistSettings, null, 2));
    await page.click('#btn-save-draft');
    await page.click('#tab-settings-diff');
    await page.click('#btn-apply-settings');
    await expect(page.locator('.modal-card')).not.toBeVisible({ timeout: 10000 });

    // 4. Place both a Manga ZIP archive and a Video MKV file into sharedWatchDir
    const mangaFile = path.join(sharedWatchDir, 'coexist_manga.zip');
    const videoFile = path.join(sharedWatchDir, 'coexist_video.mkv');
    createSampleMangaArchive(mangaFile, 6);
    createSampleVideo(videoFile, 1);

    expect(fs.existsSync(mangaFile)).toBe(true);
    expect(fs.existsSync(videoFile)).toBe(true);

    // Helpers to scan directory recursively
    const findByExt = (dir: string, ext: string) => {
      const found: string[] = [];
      const scan = (d: string) => {
        if (!fs.existsSync(d)) return;
        for (const f of fs.readdirSync(d, { withFileTypes: true })) {
          const full = path.join(d, f.name);
          if (f.isDirectory()) scan(full);
          else if (f.isFile() && f.name.endsWith(ext)) found.push(full);
        }
      };
      scan(dir);
      return found;
    };

    // 5. Verify outputs are generated in their respective directories
    await expect.poll(() => findByExt(mangaCoexistOutDir, '.cbz').length, { timeout: 30000 }).toBeGreaterThan(0);
    await expect.poll(() => findByExt(videoCoexistOutDir, '.mp4').length, { timeout: 30000 }).toBeGreaterThan(0);

    const cbzList = findByExt(mangaCoexistOutDir, '.cbz');
    const mp4List = findByExt(videoCoexistOutDir, '.mp4');
    expect(fs.statSync(cbzList[0]).size).toBeGreaterThan(0);
    expect(fs.statSync(mp4List[0]).size).toBeGreaterThan(0);

    // 6. Invariant check: input files in sharedWatchDir were NOT deleted or corrupted
    expect(fs.existsSync(mangaFile)).toBe(true);
    expect(fs.existsSync(videoFile)).toBe(true);
  });
});

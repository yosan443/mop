import { defineConfig, devices } from '@playwright/test';
import path from 'path';
import { fileURLToPath } from 'url';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

const TEST_PORT = 18999;
const TEST_DB = path.resolve(__dirname, '../target/e2e-test.db');

export default defineConfig({
  testDir: './tests',
  fullyParallel: false,
  workers: 1,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 1 : 0,
  reporter: [['list'], ['html', { open: 'never' }]],
  use: {
    baseURL: `http://127.0.0.1:${TEST_PORT}`,
    trace: 'on-first-retry',
    screenshot: 'only-on-failure',
  },
  projects: [
    {
      name: 'chromium',
      use: { ...devices['Desktop Chrome'] },
    },
  ],
  webServer: {
    command: `sh -c "rm -f ${TEST_DB}* && cargo run -p mop-cli -- serve --bind 127.0.0.1:${TEST_PORT} --db-path ${TEST_DB} --fake-backend"`,
    cwd: path.resolve(__dirname, '..'),
    url: `http://127.0.0.1:${TEST_PORT}/health`,
    reuseExistingServer: false,
    timeout: 120000,
  },
});

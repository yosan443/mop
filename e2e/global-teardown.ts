import { execSync } from 'child_process';
import fs from 'fs';
import os from 'os';
import path from 'path';

export default async function globalTeardown() {
  console.log('[E2E Teardown] Cleaning up any orphan plugin processes...');
  try {
    execSync("pkill -f '[m]op-plugin-hello' || true", { stdio: 'ignore' });
    execSync("pkill -f '[m]op-plugin-manga' || true", { stdio: 'ignore' });
    execSync("pkill -f '[m]op-plugin-video' || true", { stdio: 'ignore' });
  } catch (err) {
    console.warn('[E2E Teardown] pkill warning:', err);
  }

  try {
    const targetRunDir = path.join(os.tmpdir(), 'mop-e2e-run');
    fs.rmSync(targetRunDir, { recursive: true, force: true });
  } catch {}
}

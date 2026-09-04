import { execSync } from 'child_process';
import fs from 'fs';
import os from 'os';
import path from 'path';
import { fileURLToPath } from 'url';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

export default async function globalSetup() {
  const projectRoot = path.resolve(__dirname, '..');
  const targetPluginsDir = path.resolve(projectRoot, 'target/e2e-plugins');
  const targetRunDir = path.join(os.tmpdir(), 'mop-e2e-run');
  const helloDest = path.join(targetPluginsDir, 'mop.hello/0.1.0');

  console.log('[E2E Setup] Building mop-plugin-hello binary...');
  execSync('cargo build -p mop-plugin-hello', {
    cwd: projectRoot,
    stdio: 'inherit',
  });

  console.log('[E2E Setup] Preparing plugin directories...');
  fs.rmSync(targetPluginsDir, { recursive: true, force: true });
  fs.rmSync(targetRunDir, { recursive: true, force: true });

  fs.mkdirSync(path.join(helloDest, 'ui'), { recursive: true });
  fs.mkdirSync(targetRunDir, { recursive: true });

  const binSrc = path.join(projectRoot, 'target/debug/mop-plugin-hello');
  const binDst = path.join(helloDest, 'mop-plugin-hello');
  fs.copyFileSync(binSrc, binDst);
  fs.chmodSync(binDst, 0o755);

  fs.copyFileSync(
    path.join(projectRoot, 'plugins/hello/plugin.toml'),
    path.join(helloDest, 'plugin.toml')
  );

  fs.copyFileSync(
    path.join(projectRoot, 'plugins/hello/ui/index.js'),
    path.join(helloDest, 'ui/index.js')
  );

  console.log(`[E2E Setup] Successfully installed mop.hello at ${helloDest}`);
}

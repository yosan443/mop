import { execSync } from 'child_process';
import fs from 'fs';
import os from 'os';
import path from 'path';
import { fileURLToPath } from 'url';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

interface PluginSpec {
  id: string;
  crateName: string;
  sourceDir: string;
  version: string;
}

const PLUGINS: PluginSpec[] = [
  {
    id: 'mop.hello',
    crateName: 'mop-plugin-hello',
    sourceDir: 'plugins/hello',
    version: '1.0.0',
  },
  {
    id: 'mop.manga',
    crateName: 'mop-plugin-manga',
    sourceDir: 'plugins/manga',
    version: '1.0.0',
  },
  {
    id: 'mop.video',
    crateName: 'mop-plugin-video',
    sourceDir: 'plugins/video',
    version: '1.0.0',
  },
];

export default async function globalSetup() {
  const projectRoot = path.resolve(__dirname, '..');
  const targetPluginsDir = path.resolve(projectRoot, 'target/e2e-plugins');
  const targetRunDir = path.join(os.tmpdir(), 'mop-e2e-run');

  console.log('[E2E Setup] Building plugin binaries (hello, manga, video)...');
  execSync('cargo build -p mop-plugin-hello -p mop-plugin-manga -p mop-plugin-video', {
    cwd: projectRoot,
    stdio: 'inherit',
  });

  console.log('[E2E Setup] Preparing plugin directories...');
  fs.rmSync(targetPluginsDir, { recursive: true, force: true });
  fs.rmSync(targetRunDir, { recursive: true, force: true });
  fs.mkdirSync(targetRunDir, { recursive: true });

  for (const plugin of PLUGINS) {
    const destDir = path.join(targetPluginsDir, `${plugin.id}/${plugin.version}`);
    fs.mkdirSync(path.join(destDir, 'ui'), { recursive: true });

    const binSrc = path.join(projectRoot, `target/debug/${plugin.crateName}`);
    const binDst = path.join(destDir, plugin.crateName);
    fs.copyFileSync(binSrc, binDst);
    fs.chmodSync(binDst, 0o755);

    fs.copyFileSync(
      path.join(projectRoot, plugin.sourceDir, 'plugin.toml'),
      path.join(destDir, 'plugin.toml')
    );

    fs.copyFileSync(
      path.join(projectRoot, plugin.sourceDir, 'ui/index.js'),
      path.join(destDir, 'ui/index.js')
    );

    console.log(`[E2E Setup] Successfully installed ${plugin.id} at ${destDir}`);
  }
}

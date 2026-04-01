import { existsSync, copyFileSync } from 'fs';
import { resolve, dirname } from 'path';
import { fileURLToPath } from 'url';
import { execSync } from 'child_process';

const __dirname = dirname(fileURLToPath(import.meta.url));
const root = resolve(__dirname, '..');
const cinnyDir = resolve(root, 'cinny');

const localConfig = resolve(root, 'config.local.json');
const defaultConfig = resolve(root, 'config.json');
const dest = resolve(cinnyDir, 'config.json');

const src = existsSync(localConfig) ? localConfig : defaultConfig;
copyFileSync(src, dest);
console.log(`Copied ${src} -> ${dest}`);

// Prevent git from tracking the overwritten config.json as a change in the submodule
try {
  execSync('git update-index --skip-worktree config.json', { cwd: cinnyDir, stdio: 'ignore' });
} catch {
  // Not a git repo or git not available; safe to ignore
}

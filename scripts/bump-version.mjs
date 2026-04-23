import { readFileSync, writeFileSync } from 'node:fs';
import { resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const root = resolve(__dirname, '..');

function bumpPatch(version) {
  const parts = version.split('.');
  parts[2] = String(Number(parts[2]) + 1);
  return parts.join('.');
}

function readJSON(filePath) {
  return JSON.parse(readFileSync(filePath, 'utf-8'));
}

function writeJSON(filePath, data) {
  writeFileSync(filePath, JSON.stringify(data, null, 2) + '\n');
}

// Determine new version
const pkg = readJSON(resolve(root, 'package.json'));
const currentVersion = pkg.version;
const newVersion = process.argv[2] || bumpPatch(currentVersion);

if (!/^\d+\.\d+\.\d+/.test(newVersion)) {
  console.error(`Invalid version: ${newVersion}`);
  process.exit(1);
}

console.log(`Bumping version: ${currentVersion} -> ${newVersion}`);

// 1. package.json
const pkgPath = resolve(root, 'package.json');
pkg.version = newVersion;
writeJSON(pkgPath, pkg);
console.log(`  Updated ${pkgPath}`);

// 2. package-lock.json
const lockPath = resolve(root, 'package-lock.json');
const lock = readJSON(lockPath);
lock.version = newVersion;
if (lock.packages?.['']) {
  lock.packages[''].version = newVersion;
}
writeJSON(lockPath, lock);
console.log(`  Updated ${lockPath}`);

// 3. src-tauri/tauri.conf.json
const tauriConfPath = resolve(root, 'src-tauri/tauri.conf.json');
const tauriConf = readJSON(tauriConfPath);
tauriConf.version = newVersion;
writeJSON(tauriConfPath, tauriConf);
console.log(`  Updated ${tauriConfPath}`);

// 4. src-tauri/Cargo.toml
const cargoTomlPath = resolve(root, 'src-tauri/Cargo.toml');
let cargoToml = readFileSync(cargoTomlPath, 'utf-8');
cargoToml = cargoToml.replace(
  /^(version\s*=\s*)"[^"]*"/m,
  `$1"${newVersion}"`
);
writeFileSync(cargoTomlPath, cargoToml);
console.log(`  Updated ${cargoTomlPath}`);

// 5. src-tauri/Cargo.lock
const cargoLockPath = resolve(root, 'src-tauri/Cargo.lock');
let cargoLock = readFileSync(cargoLockPath, 'utf-8');
cargoLock = cargoLock.replace(
  /(name\s*=\s*"elevo-messenger"\nversion\s*=\s*)"[^"]*"/,
  `$1"${newVersion}"`
);
writeFileSync(cargoLockPath, cargoLock);
console.log(`  Updated ${cargoLockPath}`);

// 7. README.md download links
const readmePath = resolve(root, 'README.md');
let readme = readFileSync(readmePath, 'utf-8');
readme = readme.replace(/(elevo-messenger-v)\d+\.\d+\.\d+(\/)/g, `$1${newVersion}$2`);
readme = readme.replace(/(Elevo\.Messenger_)\d+\.\d+\.\d+(_)/g, `$1${newVersion}$2`);
writeFileSync(readmePath, readme);
console.log(`  Updated ${readmePath}`);

// 8. cinny/package.json (submodule)
const cinnyPkgPath = resolve(root, 'cinny/package.json');
const cinnyPkg = readJSON(cinnyPkgPath);
cinnyPkg.version = newVersion;
writeJSON(cinnyPkgPath, cinnyPkg);
console.log(`  Updated ${cinnyPkgPath}`);

// 9. cinny/package-lock.json (submodule)
const cinnyLockPath = resolve(root, 'cinny/package-lock.json');
const cinnyLock = readJSON(cinnyLockPath);
cinnyLock.version = newVersion;
if (cinnyLock.packages?.['']) {
  cinnyLock.packages[''].version = newVersion;
}
writeJSON(cinnyLockPath, cinnyLock);
console.log(`  Updated ${cinnyLockPath}`);

console.log(`\nDone! Version is now ${newVersion}`);

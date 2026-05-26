import { readFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';

const __dirname = dirname(fileURLToPath(import.meta.url));
const root = resolve(__dirname, '..');
const cinny = resolve(root, 'cinny');
const MAIN_BRANCH = 'main';
const RELEASE_BRANCH = 'release';
const SEMVER_RE = /\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?/;

const requestedVersion = process.argv[2];

function rel(cwd) {
  return cwd === root ? '.' : cwd.replace(`${root}/`, '');
}

function fail(message) {
  console.error(`\nRelease failed: ${message}`);
  process.exit(1);
}

function commandText(command, args) {
  return [command, ...args].join(' ');
}

function run(command, args = [], options = {}) {
  const cwd = options.cwd ?? root;
  const label = commandText(command, args);

  if (!options.quiet) {
    console.log(`\n$ (${rel(cwd)}) ${label}`);
  }

  const result = spawnSync(command, args, {
    cwd,
    encoding: 'utf-8',
    stdio: options.capture ? ['inherit', 'pipe', 'pipe'] : 'inherit',
  });

  if (result.error) {
    fail(`${label}: ${result.error.message}`);
  }

  if (result.status !== 0) {
    if (options.capture) {
      if (result.stdout) process.stdout.write(result.stdout);
      if (result.stderr) process.stderr.write(result.stderr);
    }
    fail(`${label} exited with status ${result.status}`);
  }

  return options.capture ? (result.stdout ?? '').trim() : '';
}

function capture(command, args = [], cwd = root) {
  return run(command, args, { cwd, capture: true, quiet: true });
}

function captureMaybe(command, args = [], cwd = root) {
  const result = spawnSync(command, args, {
    cwd,
    encoding: 'utf-8',
    stdio: ['inherit', 'pipe', 'pipe'],
  });

  return {
    ok: !result.error && result.status === 0,
    stdout: (result.stdout ?? '').trim(),
    stderr: (result.stderr ?? '').trim(),
    status: result.status,
    error: result.error,
  };
}

function inspectRepo(name, cwd) {
  console.log(`\n== Inspecting ${name} ==`);
  console.log(capture('git', ['status', '--short', '--branch'], cwd));
  console.log(`branch: ${capture('git', ['branch', '--show-current'], cwd)}`);
  console.log(capture('git', ['remote', '-v'], cwd));
}

function currentBranch(cwd) {
  return capture('git', ['branch', '--show-current'], cwd);
}

function requireMain(name, cwd) {
  const branch = currentBranch(cwd);
  if (branch !== MAIN_BRANCH) {
    fail(`${name} must be on ${MAIN_BRANCH}; current branch is ${branch || '(detached)'}`);
  }
}

function rootPorcelain() {
  return capture('git', ['status', '--porcelain'], root)
    .split('\n')
    .map((line) => line.trimEnd())
    .filter(Boolean);
}

function submodulePorcelain() {
  return capture('git', ['status', '--porcelain'], cinny)
    .split('\n')
    .map((line) => line.trimEnd())
    .filter(Boolean);
}

function requireCleanBeforeBump() {
  const cinnyStatus = submodulePorcelain();
  if (cinnyStatus.length > 0) {
    fail(`cinny has uncommitted changes:\n${cinnyStatus.join('\n')}`);
  }

  const rootStatus = rootPorcelain();
  const nonCinny = rootStatus.filter((line) => line.slice(3) !== 'cinny');
  if (nonCinny.length > 0) {
    fail(`root has uncommitted changes outside the cinny submodule pointer:\n${nonCinny.join('\n')}`);
  }
}

function ensureNotBehind(name, cwd) {
  run('git', ['fetch', '--prune'], { cwd });

  const upstreamResult = captureMaybe('git', ['rev-parse', '--abbrev-ref', '--symbolic-full-name', '@{u}'], cwd);
  if (!upstreamResult.ok) {
    fail(`${name} branch ${currentBranch(cwd)} has no upstream`);
  }
  const upstream = upstreamResult.stdout;

  const counts = capture('git', ['rev-list', '--left-right', '--count', 'HEAD...@{u}'], cwd);
  const [ahead, behind] = counts.split(/\s+/).map((value) => Number(value));

  if (!Number.isInteger(ahead) || !Number.isInteger(behind)) {
    fail(`${name} could not compare HEAD with ${upstream}: ${counts}`);
  }

  if (behind > 0 && ahead > 0) {
    fail(`${name} has diverged from ${upstream} (ahead ${ahead}, behind ${behind})`);
  }

  if (behind > 0) {
    run('git', ['pull', '--ff-only'], { cwd });
    return;
  }

  if (ahead > 0) {
    console.log(`${name} is ahead of ${upstream} by ${ahead} commit(s); continuing.`);
  } else {
    console.log(`${name} is up to date with ${upstream}.`);
  }
}

function parseBumpedVersion(output) {
  const doneMatch = output.match(/Done!\s+Version is now\s+(\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?)/);
  if (doneMatch) return doneMatch[1];

  const pkg = JSON.parse(readFileSync(resolve(root, 'package.json'), 'utf-8'));
  if (typeof pkg.version === 'string' && SEMVER_RE.test(pkg.version)) {
    return pkg.version;
  }

  fail('could not determine bumped version from npm output or package.json');
}

function runBump() {
  const args = ['run', 'bump'];
  if (requestedVersion) args.push(requestedVersion);

  const output = run('npm', args, { cwd: root, capture: true });
  if (output) console.log(output);

  const version = parseBumpedVersion(output);
  if (requestedVersion && version !== requestedVersion) {
    fail(`requested version ${requestedVersion}, but bump produced ${version}`);
  }
  return version;
}

function stageExisting(cwd, paths) {
  const changed = paths.filter((path) => {
    const status = capture('git', ['status', '--porcelain', '--', path], cwd);
    return status.length > 0;
  });

  if (changed.length === 0) {
    fail(`no expected bump files changed in ${rel(cwd)}`);
  }

  run('git', ['add', ...changed], { cwd });
  return changed;
}

function commitCinny(version) {
  console.log('\n== Committing cinny ==');
  console.log(capture('git', ['status', '--short'], cinny));
  stageExisting(cinny, ['package.json', 'package-lock.json']);
  run('git', ['commit', '-m', `chore: bump to ${version}`], { cwd: cinny });
  return capture('git', ['rev-parse', 'HEAD'], cinny);
}

function commitRoot(version) {
  console.log('\n== Committing root ==');
  console.log(capture('git', ['status', '--short'], root));
  stageExisting(root, [
    'package.json',
    'package-lock.json',
    'src-tauri/tauri.conf.json',
    'src-tauri/Cargo.toml',
    'src-tauri/Cargo.lock',
    'README.md',
    'cinny',
  ]);
  run('git', ['commit', '-m', `chore: bump to ${version}`], { cwd: root });
  return capture('git', ['rev-parse', MAIN_BRANCH], root);
}

function pushCurrentBranch(cwd) {
  const upstream = capture('git', ['rev-parse', '--abbrev-ref', '--symbolic-full-name', '@{u}'], cwd);
  run('git', ['push'], { cwd });
  return upstream;
}

function pushRelease(rootMainSha) {
  console.log('\n== Updating release branch ==');
  run('git', ['switch', RELEASE_BRANCH], { cwd: root });
  run('git', ['reset', '--hard', rootMainSha], { cwd: root });
  const upstream = pushCurrentBranch(root);
  return upstream;
}

function main() {
  inspectRepo('root', root);
  console.log(`\nsubmodule: ${capture('git', ['submodule', 'status'], root)}`);
  console.log(`cinny from root: ${capture('git', ['-C', 'cinny', 'status', '--short', '--branch'], root)}`);
  inspectRepo('cinny', cinny);

  requireMain('root', root);
  requireMain('cinny', cinny);
  requireCleanBeforeBump();

  ensureNotBehind('root', root);
  ensureNotBehind('cinny', cinny);

  const version = runBump();
  const cinnySha = commitCinny(version);
  const rootMainSha = commitRoot(version);

  console.log('\n== Pushing cinny main ==');
  const cinnyUpstream = pushCurrentBranch(cinny);
  const releaseUpstream = pushRelease(rootMainSha);

  console.log('\nRelease complete');
  console.log(`version: ${version}`);
  console.log(`cinny commit: ${cinnySha}`);
  console.log(`root main commit: ${rootMainSha}`);
  console.log(`pushed cinny: ${cinnyUpstream}`);
  console.log(`pushed release: ${releaseUpstream}`);
}

main();

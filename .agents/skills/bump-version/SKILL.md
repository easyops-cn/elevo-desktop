---
name: bump-version
description: Run the Elevo/Cinny desktop version bump and release publishing workflow. Use when the user asks to bump the project version, publish a release branch, run `npm run bump`, or update and push both the root cinny-desktop repository and its `cinny` submodule.
---

# Bump Version

Execute the release bump workflow for `/Users/wangshenwei/github/cinny-desktop` and its `cinny` git submodule. Treat this as a live release operation: stop on dirty worktrees, non-fast-forward divergence, failed commands, or any mismatch in the detected version.

## Inputs

- Optional version argument from the user, e.g. `1.2.3`.
- If no version is provided, run the bump command without an argument and use the version it reports.

## Required Repository

- Root repository: `/Users/wangshenwei/github/cinny-desktop`
- Submodule repository: `/Users/wangshenwei/github/cinny-desktop/cinny`
- Main branch name: `main`
- Release branch name in the root repository: `release`

## Workflow

1. Inspect both repositories before changing anything.
   - In root and submodule, run `git status --short --branch`, `git branch --show-current`, and `git remote -v`.
   - In root, also run `git submodule status` and `git -C cinny status --short --branch`.
   - Require both repositories to be on `main`.
   - Allow either repository to have local commits ahead of upstream.
   - Require the submodule worktree to be clean before bumping.
   - In the root repository, require no uncommitted changes except a `cinny` submodule commit-pointer change where the submodule worktree itself is clean. This appears as a root `cinny` change, but it is acceptable when it only records that the submodule HEAD differs from the root index.
   - If root has any non-`cinny` uncommitted changes, or if `cinny` has modified/untracked files, stop and ask.

2. Ensure root and submodule `main` are not behind remote.
   - Run `git fetch --prune` in each repository.
   - Determine the upstream with `git rev-parse --abbrev-ref --symbolic-full-name @{u}`.
   - Compare local and upstream with `git rev-list --left-right --count HEAD...@{u}`.
   - If local is behind and not ahead, fast-forward with `git pull --ff-only`.
   - If local is ahead and not behind, continue without pulling; unpushed local commits are allowed.
   - If local is both ahead and behind, stop because the branch has diverged and cannot be fast-forwarded safely.
   - If a repository has no upstream, stop and report the exact repository and branch state.

3. Run the bump command in the root repository.
   - Without a user version: `npm run bump`
   - With a user version: `npm run bump VERSION`
   - Capture the created version from command output. Prefer the final line format `Done! Version is now VERSION`.
   - If the output does not contain an unambiguous semantic version, inspect `package.json` and confirm the version before proceeding.

4. Commit the submodule changes first.
   - In `/Users/wangshenwei/github/cinny-desktop/cinny`, review `git status --short`.
   - Stage only the bump-related version files changed by `npm run bump`.
   - Commit with exactly `chore: bump to VERSION`.

5. Commit the root repository changes.
   - In `/Users/wangshenwei/github/cinny-desktop`, review `git status --short`.
   - Include root bump files and the updated `cinny` submodule pointer.
   - Commit with exactly `chore: bump to VERSION`.

6. Push the submodule main branch first.
   - Push from `/Users/wangshenwei/github/cinny-desktop/cinny` to its upstream `main`.
   - Stop if the push fails.

7. Update and push the root release branch.
   - In `/Users/wangshenwei/github/cinny-desktop`, save the current `main` commit SHA with `git rev-parse main`.
   - Switch to `release`.
   - Force reset the local `release` branch to the just-recorded local `main` commit with `git reset --hard main`.
   - Push `release` to its upstream remote.
   - Do not use a force push unless the normal push is rejected and the user explicitly approves it.

## Safety Rules

- Do not continue after a failed command.
- Do not merge or rebase as part of this workflow.
- Do not use `git reset --hard` anywhere except the explicit root `release` branch reset to local `main`.
- Do not push the root `main` branch unless the user explicitly asks; this workflow pushes the `cinny` submodule first and then the root `release` branch.
- Do not reject a clean local-ahead `main` branch; only reject behind/diverged states that cannot be resolved with `git pull --ff-only`.
- Do not treat a root `cinny` submodule pointer change as dirty by itself when `git -C cinny status --short` is clean.
- If remote names differ from expectations, rely on each branch's configured upstream rather than guessing a remote.
- Report the final version, submodule commit SHA, root main commit SHA, and pushed release branch when complete.

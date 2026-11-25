# Maintaining Transverse

Notes for maintainers on branches, pull requests and releases. Contributors only need [CONTRIBUTING.md](CONTRIBUTING.md).

## Branches and rules

| Branch | Receives                                 | Merge method          |
| ------ | ---------------------------------------- | --------------------- |
| `dev`  | all pull requests (from branches/forks)  | squash and merge      |
| `main` | only the release pull request from `dev` | create a merge commit |

Both branches are protected by rulesets: no direct pushes, no force pushes, no deletion, pull request required, only the stated merge method allowed, and the status checks below must pass. The bypass list is empty. The rulesets are stored in `.github/rulesets/` and can be imported (see "Setting up the repository").

| Check           | Workflow            | Required on   |
| --------------- | ------------------- | ------------- |
| `check`         | `ci.yml`            | `dev`, `main` |
| `build`         | `ci.yml`            | `dev`, `main` |
| `pr-title`      | `pr-title.yml`      | `dev`         |
| `source-branch` | `source-branch.yml` | `main`        |

- `check` is a small job that waits for three parallel jobs: `rust` (`cargo fmt --check`, clippy with `-D warnings`, the tests including the parity tests against the Python reference), `licenses` (the dependency license check with `cargo deny`, allowlist in `deny.toml`, and the generation of the third-party notices) and `scripts` (the tests of the release script and the checks of the Python reference implementation: ruff, pytest). It runs on every pull request and on pushes to `dev` and `main` (feature branches are only built through their pull request, so open it early, as a draft). Require only `check`, not the individual jobs.
- `build` runs for `main`, `dev` and pull requests targeting them. It is a small job that waits for three build jobs that run in parallel with `check` (not after it): `build (linux)` (glibc 2.28) and `build (windows)` (release builds of the desktop app with zig) and `build (web)` (the core for WebAssembly). The installable packages are only built by the release workflow. Require only `build`, not the individual jobs.
- `pr-title` validates that the PR title is a conventional commit.
- `source-branch` fails for pull requests into `main` that don't come from `dev` of this repository.
- Require the `(pull_request)` runs, not the `(push)` runs, where a check exists for both.
- The Rust build cache is only written by runs on `dev` (`save-if` in `ci.yml`); pull requests and `main` read it. This keeps the repository's cache quota free for the cache that is actually reused.
- Do not enable "Require branches to be up to date" for `main`. After a release, `dev` is one merge commit behind `main`, and that setting would force a back-merge each time.

## Reviewing and merging a pull request

1. Check that the PR does **one thing**. If not, ask for a split. A squash commit with a good title is the goal.
2. Check the title: the right type, a scope if useful, and a description that reads well in release notes. Fix the title yourself if needed.
3. Check for breaking changes. They need `!` in the title, otherwise the version bump will be wrong.
4. Squash and merge. The commit message is the PR title (with the PR number appended).

Because the version and the notes are derived from the squash commit, a wrong title cannot be fixed by editing it after the merge.

## Releasing

Everything except the final merge is automatic.

1. Whenever `dev` contains a releasable change (`feat`, `fix`, `perf` or a breaking change since the last release), the `Release preview` workflow opens a **draft** pull request `chore(release): next release` from `dev` to `main`. On every further push to `dev` it updates the description with the next version and the notes. With only `chore`, `ci`, `docs`, `refactor`, `test` or `build` changes, no PR is opened.
2. When you want to release, review the version and notes in the description, mark the PR **Ready for review**, wait for the checks and merge it with a **merge commit**.
3. The `Release` workflow runs on the push to `main`. It computes the version and the notes again, builds the desktop app for Linux and Windows, runs the self-test of the Windows build on a Windows runner, and only when everything succeeded creates the tag and the GitHub release (with the notes and all files). If a build fails, nothing is released: fix the cause, or re-run the failed jobs from the Actions tab, and the release follows.

The release contains:

| File                                      | Content                                                |
| ----------------------------------------- | ------------------------------------------------------ |
| `transverse-X.Y.Z-linux-x86_64.tar.gz`    | Linux (glibc 2.28 or newer): `transverse-gui`, CLI     |
| `transverse-X.Y.Z-windows-x86_64.zip`     | Windows: `transverse-gui.exe`, `transverse.exe` (CLI)  |
| `SHA256SUMS.txt`                          | checksums of the files above                           |

The binaries are **not code signed**: Windows shows a SmartScreen warning ("More info", then "Run anyway"). There is no macOS or arm64 build yet. The web version is prepared (`web` job in the release workflow, `deploy-web` in phase W of the plan) but publishes nothing yet.

**Dry run:** to test the builds without releasing, run the `Release` workflow manually (Actions tab → Release → Run workflow, optionally with a dummy version). It builds everything with the dummy version and keeps the files as workflow artifacts for 3 days. It creates no tag and no release. Do this after changing the build scripts or the release workflow.

No commit is made for a release: the workspace version in `Cargo.toml` is the placeholder `0.0.0` and is intentionally never changed. The tag is the source of truth. The build jobs run `scripts/set-version.sh <version>` in the CI workspace before building, so only the released build contains the real version. Local and development builds show `v0.0.0`. Do not commit a version bump.

### Version rules

The version is calculated by `scripts/release.mjs` from the squashed commit titles since the latest `vX.Y.Z` tag (or from the beginning of the history for the very first release, which starts from 0.0.0):

| Base       | breaking | `feat` | `fix` / `perf` |
| ---------- | -------- | ------ | -------------- |
| 0.x        | minor    | minor  | patch          |
| 1.0 and up | major    | minor  | patch          |

Other types never release. A release is only created if at least one `feat`, `fix`, `perf` or breaking commit is included. The preview in the PR description shows exactly what would be released. Commits whose title is not a conventional commit are silently ignored, so if something is missing from the preview, check its title.

### Going to 1.0 (or overriding a version)

Add a label named `release-as:X.Y.Z` (for example `release-as:1.0.0`) to the release PR **before merging**. The description updates to the new version and the release uses it. The version must be greater than the latest release, otherwise the workflow fails with an error and nothing is released. The label can be created on the fly in the PR sidebar.

### Fixing a release PR

- **Wrong version or notes:** a title was wrong. The merged commit cannot be changed. Override the version with `release-as`, or edit the notes of the GitHub release after it has been created.
- **Release notes of an old commit are missing:** the commit title wasn't conventional. Add an empty commit with a proper `fix:` or `feat:` title through a PR.
- **Release failed:** check the `Release` workflow run. Nothing is created until the last step, so fix the cause and re-run the workflow.

## Setting up the repository

One-time setup, in this order (the rulesets come last, because they block the direct pushes needed to bootstrap):

1. **Branches:** push `main` and `dev` (both at the same commit). Set **`dev` as the default branch** (Settings → General → Default branch), so new pull requests target it by default.
2. **Merge settings** (Settings → General → Pull Requests): allow squash merging (default message: the PR title) and merge commits. Rebase merging can stay disabled. The rulesets restrict `dev` to squash and `main` to merge commits.
3. **Actions permissions** (Settings → Actions → General): workflow permissions "Read repository contents" is enough. To let the `Release preview` workflow open the release PR with the default token, enable "Allow GitHub Actions to create and approve pull requests" (or set up the GitHub App below).
4. **Rulesets** (Settings → Rules → Rulesets → New ruleset → Import a ruleset): import `.github/rulesets/main.json` and `.github/rulesets/dev.json`. Check that the status checks `check`, `build`, `pr-title` and `source-branch` can be found (they appear after the workflows have run once; choose the `pull_request` runs where there are two).
5. **Optional, GitHub App for the release PR:** see below.

Rulesets (like branch protection) are only enforced for private repositories on a paid GitHub plan. On the free plan the workflows still run and report, but nothing blocks a merge.

- **GitHub App for the release PR:** the `Release preview` workflow can open and update the release PR with a short-lived token of a GitHub App, so the PR is authored by the app (`<app name>[bot]`), not by a person. PRs opened with the default `GITHUB_TOKEN` don't run the pull request workflows, so the required checks would never report on them (a closed and reopened PR does). The app needs the repository **variable** `RELEASE_APP_CLIENT_ID` (the app's client ID) and the repository **secret** `RELEASE_APP_PRIVATE_KEY` (the full content of a private key generated for the app), with the repository permissions _Pull requests: read and write_ and _Contents: read_ (nothing else), installed on this repository only. Without the variable the workflow falls back to `GITHUB_TOKEN`. The token is created per run and expires after an hour, so there is nothing to renew.
- **Dependabot** (`.github/dependabot.yml`) opens grouped monthly dependency updates against `dev` for the Rust crates, the Python environment and the GitHub actions. `faer` (pinned, sparse API changes), and `eframe` / `egui_plot` (0.x releases change the API and have to move together) are updated manually.

## License

Transverse is licensed under the GPL-3.0-only (`LICENSE`); contributors agree to the CLA (`CLA.md`), which lets the project owner relicense contributions later. The release archives contain `LICENSE` and `THIRD_PARTY_LICENSES.txt`, the notices and license texts of all bundled dependencies, which the release build generates with `cargo about` (`about.toml`, `about.hbs`; keep the accepted licenses in sync with `deny.toml`; the file is not committed, so it never goes stale). `cargo deny check licenses` fails when a dependency (also after a dependabot update) brings a license that is not on the allowlist in `deny.toml`: check that it is compatible with the GPL-3.0 before adding it there. Note that crates offering a choice (e.g. `Apache-2.0 OR GPL-2.0-only`) are fine as long as an allowed alternative exists; GPL-2.0-only code on its own would not be compatible.

## Workflows

| File                  | Trigger                          | Purpose                                                          |
| --------------------- | -------------------------------- | ---------------------------------------------------------------- |
| `ci.yml`              | pull requests, push to dev/main  | format, lints, tests, licenses, Python, builds (incl. wasm) |
| `pr-title.yml`        | pull requests                    | conventional commit check for the PR title                       |
| `source-branch.yml`   | pull requests into `main`        | only `dev` may be merged into `main`                             |
| `release-preview.yml` | push to `dev`, release PR events | opens/updates the draft release PR                               |
| `release.yml`         | push to `main`                   | version, tag, GitHub release, Linux and Windows builds           |

When changing the release logic, `scripts/release.mjs` can be run locally: `node scripts/release.mjs --ref origin/dev --preview` prints the next version and the notes without changing anything, and `node --test scripts/release.test.mjs` runs its tests.

## Building a release locally

`scripts/build-release.sh [linux|windows|all]` builds and packages like the release workflow does (without a real version). It needs zig and cargo-zigbuild; the script lists what is missing.

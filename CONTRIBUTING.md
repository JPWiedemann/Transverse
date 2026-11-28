# Contributing to Transverse

This page explains how pull requests and releases work in this repository.

Transverse is licensed under the [GPL-3.0](LICENSE). By submitting a contribution you agree to the [Contributor License Agreement](CLA.md).

## Opening a pull request

1. Create a branch (or a fork) from **`dev`**.
2. Make your change and test it: for GUI changes run the app (`cargo run --release -p transverse-gui`).
3. Open the pull request against **`dev`**.

Pull requests into `main` are rejected: `main` only receives the release pull request from `dev`, which is opened by a bot and merged by the maintainer. Direct pushes to `main` and `dev` are not possible.

### One pull request, one change

Each pull request should contain **a single feature or a single bug fix**. Pull requests are squash merged, so the whole PR becomes one commit, and its title becomes both the commit message and the entry in the release notes. A PR that does several things cannot have a good title, so it will be asked to be split up.

### The pull request title

The title **must follow the [Conventional Commits](https://www.conventionalcommits.org/) format**. A check on the pull request validates it, and you can edit the title at any time (the check re-runs):

```
<type>(<optional scope>): <short description>
```

Write the description for the people reading the release notes: what changes for users, in the imperative mood, without a trailing period. Useful scopes are the crates and areas: `core`, `lines`, `gui`, `cli`, `python`, `web`.

| Type                                               | Use for                                                          | Release                                 |
| -------------------------------------------------- | ---------------------------------------------------------------- | --------------------------------------- |
| `feat`                                             | a new feature                                                    | minor version                           |
| `fix`                                              | a bug fix                                                        | patch version                           |
| `perf`                                             | a performance improvement                                        | patch version                           |
| `docs`, `refactor`, `test`, `build`, `ci`, `chore` | changes that don't affect users                                  | none, not listed in the release notes   |
| any of the above, with `!` (e.g. `feat(core)!: …`) | a breaking change (for example one that changes the file format) | minor version below 1.0, major from 1.0 |

Examples:

- Good: `fix(gui): readout shows V/mm instead of V/m`
- Good: `feat(lines): coaxial line with analytic Z0`
- Good: `refactor(core): split the FD assembly`
- Bad: `fixed stuff` (no type)
- Bad: `feat: various improvements and fixes` (several changes in one)
- Bad: `fix: bug` (says nothing in the release notes)

Mark breaking changes with `!` in the title and describe the impact in the PR description.

### Checks

Before pushing, run what every pull request has to pass:

```sh
cargo fmt --all                                        # formatting (CI: cargo fmt --all --check)
cargo clippy --release --all-targets -- -D warnings    # lints
cargo test --release                                   # includes the parity tests against the Python reference
cargo check --target wasm32-unknown-unknown -p transverse-core -p transverse-lines   # web portability (CI builds and tests the whole module)
scripts/build-web.sh && node web/tests/reference.mjs    # wasm module and the reference cases through Node (needs wasm-bindgen-cli, see the script)
(cd web && npm ci && npm run check && npm test)          # web frontend: type check and tests; npm run dev starts it
(cd web && npx playwright install chromium && npm run build && npm run e2e)   # browser tests of the built site (first time: install the browser)
node --test scripts/release.test.mjs                                   # release script tests (only if you touched scripts/)
cd python && uv run --group dev ruff check . && uv run --group dev ruff format --check . && uv run --group dev pytest   # only for Python changes
```

The pull request cannot be merged until the checks `check`, `build` and `pr-title` pass. `build` makes the release builds of the desktop app for Linux and Windows (and the core for WebAssembly), so linker and cross-compilation problems show up before a release. To try it locally: `scripts/build-release.sh` (needs zig and cargo-zigbuild, see the script). Your branch must also be up to date with `dev`: if another pull request was merged in the meantime, use **Update branch** on the pull request (merging `dev` into your branch is enough, no rebase needed) and wait for the checks again. You don't need to clean up your commits, because the PR is squash merged.

## How releases work

You don't need to do anything for releases, but it explains why the title matters:

- Merged pull requests collect on `dev`.
- A bot keeps a draft pull request from `dev` to `main` whose description shows the next version and the release notes. Both are generated from the titles of the merged pull requests.
- When the maintainer merges that pull request, a release with the Linux and Windows builds attached is created automatically.

Versions follow [semantic versioning](https://semver.org/). Maintainer details are in [MAINTAINING.md](MAINTAINING.md).

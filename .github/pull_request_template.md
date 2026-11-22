<!--
The PR title becomes the squash commit and the release note, so it must be a conventional commit:
  <type>(<optional scope>): <short description>
Types: feat, fix, perf (released), docs, refactor, test, build, ci, chore (not released).
Mark breaking changes with "!" (e.g. feat(core)!: ...). Example: fix(gui): readout shows V/mm instead of V/m
See CONTRIBUTING.md for details.
-->

## What does this change?

## Checklist

- [ ] This PR contains a single feature or bug fix
- [ ] I agree to the [Contributor License Agreement](https://github.com/JPWiedemann/Transverse/blob/dev/CLA.md)
- [ ] The PR targets `dev` and the title is a conventional commit
- [ ] I tested the change (in the app, if it affects the GUI)
- [ ] `cargo fmt --all --check`, `cargo clippy --release --all-targets -- -D warnings` and `cargo test --release` pass

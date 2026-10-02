# Project conventions

Apply these conventions to future changes, including locally built updates before a release pipeline exists.

## Versioning

Follow [Semantic Versioning 2.0.0](https://semver.org/spec/v2.0.0.html). Treat documented CLI options, configuration, keyboard controls, and persisted user data as compatibility surfaces.

- Bump the app version for each completed app update delivered to the user. Choose the highest applicable bump for the combined changes; intermediate implementation commits do not each need a bump.
- Backward-compatible bug fixes, performance improvements, and internal maintenance shipped in the app use a patch bump.
- Backward-compatible features and deprecations use a minor bump, resetting the patch component.
- From `1.0.0` onward, incompatible changes use a major bump, resetting minor and patch components.
- While the project is `0.y.z`, our project convention is to use minor bumps for features or incompatible changes, and patch bumps for compatible fixes. Clearly document breaking changes and upgrade steps even during this initial-development period.
- Changes limited to documentation, tests, or development tooling do not require a bump unless they change the delivered app's behavior or compatibility.
- Update `[package].version` in `Cargo.toml` and the root `whatsapp-tui` package version in `Cargo.lock` together, preserving unrelated dependency versions.
- Include the bump before the final build and verification. Check that the rebuilt binary's `--version` matches the manifest, and report the new version in the handoff.

## Commit subjects

- Start the first word in lowercase unless its correct spelling requires capitalization, such as a proper name or acronym.
- Preserve correct capitalization elsewhere, including WhatsApp, GIF, GNOME, Rust, and API. Do not force the entire subject to lowercase.
- Keep subjects concise and describe the change. Examples: `add inline GIF playback`, `fix muted group notifications`, `document versioning conventions`.

## Development checks

Enable the checked-in pre-push hook once per clone:

```sh
git config --local core.hooksPath .githooks
```

Before each push, it checks formatting, runs Clippy with warnings denied, runs all test targets, and builds the release binary. A failure stops the push. The shared runner is also used by CI, with the toolchain from `rust-toolchain.toml` and locked dependencies. Build jobs and test threads default to two; `CARGO_BUILD_JOBS` and `RUST_TEST_THREADS` can override those limits.

Run the same checks manually with `./scripts/check.sh`. Use `./scripts/check.sh checks` for formatting/lint/tests only, or `./scripts/check.sh release` for just the optimized build. CI retains its artifact cleanup before the release build to stay within the runner's disk budget; local checks reuse the Cargo cache.

The hook requires a clean checkout of the commit being pushed, so a passing result covers that code. Commit or stash changes first. Branches and annotated tags pointing to the checked-out commit are checked once; pushing another revision requires checking it out first. Deletion-only and empty pushes skip validation. Hook configuration is local to the clone and is not installed automatically by Git.

PTY tests match text on a virtual terminal screen so cursor movements, retained spaces, and partial redraws do not cause false failures. Graphics and terminal-restoration checks continue to inspect the raw control sequences.

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

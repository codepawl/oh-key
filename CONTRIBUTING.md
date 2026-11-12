# Contributing to oh-key

Thanks for helping. The most useful things right now are typing bug reports and test cases.

## Report a bug

Open an issue with the bug report form. Include:

- The version: `oh-key-cli --version`.
- Your system and the application where it happens.
- The exact keys you pressed, the text you expected and the text you got.

## Send a change

1. Open an issue first for a large change.
2. Fork the repository and branch from `main`.
3. Run these and fix what they report:
   - `cargo fmt --check`
   - `cargo clippy --workspace -- -D warnings`
   - `cargo test --workspace`
4. Open a pull request that says what changed and how you checked it.

Engine changes come with a test: the keys typed and the text expected.

## Where the code is

- `crates/oh-key-charset`: charset conversion.
- `crates/oh-key-engine`: the typing engine and spell check.
- `crates/oh-key-macro`: macro expansion.
- Frontends (the Windows tray app, later macOS and Linux) sit in their own crates and call the engine.

## Style

- Use full, descriptive names. Do not abbreviate.
- Keep functions small, and put each statement on its own line.
- Write a comment only where the reason is not obvious from the code.

By contributing, you agree that your work is licensed under GPL-3.0-or-later, the licence of this repository.

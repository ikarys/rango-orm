# Contributing to Rango

Thanks for helping. Rango follows the [Vibecoding Convention](VIBECODING.md) at level 2; this page
is what that means for a contribution.

## Workflow

- Branch from `develop` (`feature/*`, `fix/*`, `docs/*`) and open the pull request against `develop`.
- Before asking for review: `just check` (format, clippy) and the tests for what you touched
  (`cargo test --workspace`; `just test-all` runs every database, start them with `just dev`).
- `just hooks` once per clone turns on the pre-commit secrets scan (needs
  [gitleaks](https://github.com/gitleaks/gitleaks)).
- Deleting, skipping or weakening a test needs the reason in the commit message.
- A new dependency must earn its place: say in the pull request why, and check it on crates.io
  (exact name, maintained, current version).

## AI-assisted contributions

Accepted, with disclosure:

- Each commit with AI-written content ends with one trailer per model:
  `Assisted-by: <Tool>:<model>`, for example `Assisted-by: Claude Code:claude-opus-5-5`.
- You know every line of the diff and can say what it does, why, and what it affects.
- The pull request shows what you ran and its result (the template asks for it).

A pull request that hides AI assistance, or whose author cannot explain it, is closed.

## Security

Report vulnerabilities privately, never in an issue: see [SECURITY.md](SECURITY.md).

## Licence

Contributions are dual licensed under MIT OR Apache-2.0, as stated in the [README](README.md#license).

Maintainer: Ikarys <bikarys@proton.me>.

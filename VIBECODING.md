---
# AI involvement, AI-DECLARATION.md keys (https://github.com/DimwitLabs/AI-DECLARATION.md)
version: "0.1.1"
level: copilot
processes:
  design: pair
  implementation: copilot
  testing: copilot
  documentation: copilot
  review: hint
  deployment: none
# Vibecoding Convention (https://github.com/ikarys/vibecoding-convention)
convention:
  version: "0.1"
  level: 2
  adopted_at: 765491d
  status: active
  tools:
    - "Claude Code:claude-fable-5-1"
    - "Claude Code:claude-opus-5-5"
    - "Claude Code:claude-sonnet-5-5"
    - "local:qwen3.8-27b"
  attribution_trailers: [Assisted-by]
  maintainers: [ikarys]
  review: "single maintainer reads every diff; agents work on short-lived branches and merge into develop only with tests and lint green, releases to main only with the maintainer's go-ahead"
  security_response: "acknowledged within 7 days"
  licence: MIT OR Apache-2.0
  coverage: { threshold: 75, kind: line, tool: cargo-llvm-cov }
  maintainability: { tool: "jscpd (duplication), clippy cognitive_complexity (complexity), vibecheck (function and file length, markers)", duplication_max: 14, complexity_max: 35, function_max: 60, file_max: 2600, markers_max: 260 }
  freshness: { max_outdated: 5 }
  evidence: { coverage: audit/evidence/coverage.txt, audit: audit/evidence/audit.json, outdated: audit/evidence/outdated.txt, maintainability: audit/evidence/jscpd-report.json }
  commands:
    test: "cargo test --workspace"
    coverage: "cargo llvm-cov --workspace --features rango-tests/integration,rango-tests/integration-sqlite,rango-orm-cli/integration --summary-only -- --test-threads=1"
    lint: "cargo clippy --workspace --all-targets --all-features -- -W clippy::cognitive_complexity -D warnings"
    format: "cargo fmt --all -- --check"
    audit: "cargo audit"
    static_analysis: "cargo clippy --workspace --all-targets --all-features -- -W clippy::cognitive_complexity -D warnings"
    outdated: "cargo outdated --workspace --root-deps-only"
    secrets: "gitleaks git --no-banner --redact ."
    maintainability: "npx -y jscpd@4 rango-orm-core/src rango-orm-derive/src rango-orm-postgres/src rango-orm-sqlite/src rango-orm-mysql/src rango-orm/src rango-orm-cli/src --silent --reporters json --output audit/evidence"
    doc_lint: "cargo +stable doc --workspace --all-features --no-deps"
---

# Vibecoding declaration

This project follows the [Vibecoding Convention](https://github.com/ikarys/vibecoding-convention)
version 0.1 at level 2. How each quality claim is checked is in
[How Rango is built and verified](docs/src/trust/how-rango-is-built.md); the security claims are in
[SECURITY.md](SECURITY.md) and the [security requirements](docs/src/trust/security-requirements.md).

## AI assistance

Most of Rango's code and documentation is AI-written. The maintainer frames each change, reads
it and decides; Claude Code (Opus, at times Fable) frames, reviews and fixes, Sonnet subagents and a local model
(qwen3.8-27b) write bulk code. Blind usability tests are run by AI agents that only read the user
docs; their reports and the maintainer's decisions are in [audit/](audit/README.md). Every
AI-written commit carries an `Assisted-by: <tool>:<model>` trailer.

## Review

Single maintainer. Work happens on short-lived branches from `develop`; a branch is merged into
`develop` (`--no-ff`) only when `just check` (format, clippy) and `just test-all` (unit, SQLite,
PostgreSQL, MySQL and MariaDB integration, CLI end-to-end) pass, and the maintainer reads the
diff. CI runs the same suites, split by database, on every push to `develop` and `main` and on
pull requests. Merging into `main` and publishing to crates.io are done by the maintainer only.

## Thresholds

- Coverage 75 % (lines, `cargo llvm-cov`, 77.2 % at adoption), under the recommended 80 %:
  the uncovered lines are mostly MySQL-only paths, which CI covers in its own job but the
  coverage run (PostgreSQL and SQLite) does not reach.
- Duplication 14 % (jscpd, 13.1 % at adoption): the three backends repeat the same shape of
  binding, decoding and execution code on purpose, one crate per driver.
- Complexity 35 (clippy `cognitive_complexity`, highest 32 at adoption), function 60 lines and
  file 2600 lines (vibecheck, 50 and 2592 at adoption; the long files are integration test
  suites).
- Markers 260 (vibecheck, 254 at adoption, no `TODO` or `FIXME`): the counter matches the word
  "placeholder", a core term of an ORM (SQL bind placeholders), and reads rustdoc examples as
  commented-out code.
- Outdated dependencies 5 (`cargo outdated` on direct dependencies, 0 at adoption).

## Provenance and licence

Rango is licensed under MIT OR Apache-2.0. Parts of it are written by AI assistants and may not be
protected by copyright in some jurisdictions; the maintainer accepts this. To the maintainer's
knowledge, no verbatim reproduction of licence-incompatible code was included. Safeguards used: the
maintainer reads every diff; `cargo deny check` refuses a dependency under a licence outside the
allow list in `deny.toml`.

## Status

Active, pre-1.0. Vulnerabilities are reported privately on GitHub and acknowledged within 7 days
([SECURITY.md](SECURITY.md)).

## Exceptions

- D3: commits before 2026-11-19 carry `Co-Authored-By: Claude ...` instead of `Assisted-by`
  (no model name), and the earliest ones none; they were written with AI assistance too. The
  `Assisted-by` trailer is required from adoption on. Declared 2026-11-20.

## Dependencies held back

None.

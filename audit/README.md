# Blind test reports

Reports from the usability tests described in [Blind usability tests](../docs/src/trust/blind-tests.md). Each report is kept as the tester wrote it, with a header and, at the end, a maintainer decision for every finding.

| Date | Report | Campaign | Commit | Backends | Score | Open findings |
|---|---|---|---|---|---|---|
| 2026-11-08 | [run 1](2026-11-08-run1-library-django.md) | A (earlier protocol) | 4d109f7 | SQLite, PostgreSQL | 7/10 | 3 |
| 2026-11-08 | [run 2](2026-11-08-run2-library-django.md) | A (earlier protocol) | about 9860de9 | SQLite, PostgreSQL | 8/10 | 3 |

## Open findings

Run 1 ([report](2026-11-08-run1-library-django.md#f-decisions)):

- #9: "What gets detected" table in makemigrations.md has no rename, constraint or index rows.
- #18: `CheckConstraint::raw` in the docs does not exist (it is `new`); partial unique is documented as Postgres-only but SQLite emits it too.
- #20: quick-start teaches string column names before typed columns.

Run 2 ([report](2026-11-08-run2-library-django.md#f-decisions)):

- #5: `connect_sqlite(&str)` vs `connect_postgres(&DatabaseConfig)`; quick-start shows no Postgres call.
- #9: SQLite bulk chunk size still 999 parameters.
- #11: no note that `FieldPassword` hashing is slow in debug builds.

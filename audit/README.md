# Blind test reports

Reports from the usability tests described in [Blind usability tests](../docs/src/trust/blind-tests.md). Each report is kept as the tester wrote it, with a header and, at the end, a maintainer decision for every finding.

| Date | Report | Campaign | Commit | Backends | Score | Open findings |
|---|---|---|---|---|---|---|
| 2026-11-08 | [run 1](2026-11-08-run1-library-django.md) | A (earlier protocol) | 4d109f7 | SQLite, PostgreSQL | 7/10 | 0 |
| 2026-11-08 | [run 2](2026-11-08-run2-library-django.md) | A (earlier protocol) | about 9860de9 | SQLite, PostgreSQL | 8/10 | 0 |
| 2026-11-08 | [campaign A](2026-11-08-campaign-a-library-django.md) ([brief](2026-11-08-campaign-a-library-django.brief.md)) | A: library, Django developer | bd5de84 | SQLite, PostgreSQL | 7/10 | 1 |
| 2026-11-08 | [campaign B](2026-11-08-campaign-b-saas-billing-pg.md) ([brief](2026-11-08-campaign-b-saas-billing-pg.brief.md)) | B: SaaS billing, backend engineer | bd5de84 | PostgreSQL | 7/10 | 2 |
| 2026-11-08 | [campaign C](2026-11-08-campaign-c-event-booking-mysql.md) ([brief](2026-11-08-campaign-c-event-booking-mysql.brief.md)) | C: event booking, indie developer | bd5de84 | MySQL 8.4, MariaDB 11.4, SQLite | 6.5/10 | 4 |

## Open findings

None is a `BLOCKER` or `DOC-WRONG`. All are planned after 0.2.0.

- Values of the wrong type passed with a string column name are not checked against the schema type (A #12, B #13, C #13). Typed columns catch it at compile time.
- No GROUP BY in the query builder (B #14).
- `raw` maps rows to models only (C #18).
- No typed error for numeric overflow (C #20).
- Two slow-query `WARN` events for one statement on MySQL, not reproduced yet (C #21).

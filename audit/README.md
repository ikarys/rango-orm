# Blind test reports

Reports from the usability tests described in [Blind usability tests](../docs/src/trust/blind-tests.md). Each report is kept as the tester wrote it, with a header and, at the end, a maintainer decision for every finding.

| Date | Report | Campaign | Commit | Backends | Score | Open findings |
|---|---|---|---|---|---|---|
| 2026-11-08 | [run 1](2026-11-08-run1-library-django.md) | A (earlier protocol) | 4d109f7 | SQLite, PostgreSQL | 7/10 | 0 |
| 2026-11-08 | [run 2](2026-11-08-run2-library-django.md) | A (earlier protocol) | about 9860de9 | SQLite, PostgreSQL | 8/10 | 0 |
| 2026-11-08 | [campaign A](2026-11-08-campaign-a-library-django.md) ([brief](2026-11-08-campaign-a-library-django.brief.md)) | A: library, Django developer | bd5de84 | SQLite, PostgreSQL | 7/10 | 1 |
| 2026-11-08 | [campaign B](2026-11-08-campaign-b-saas-billing-pg.md) ([brief](2026-11-08-campaign-b-saas-billing-pg.brief.md)) | B: SaaS billing, backend engineer | bd5de84 | PostgreSQL | 7/10 | 2 |
| 2026-11-08 | [campaign C](2026-11-08-campaign-c-event-booking-mysql.md) ([brief](2026-11-08-campaign-c-event-booking-mysql.brief.md)) | C: event booking, indie developer | bd5de84 | MySQL 8.4, MariaDB 11.4, SQLite | 6.5/10 | 4 |
| 2026-11-20 | [campaign A](2026-11-20-campaign-a-library-django.md) ([brief](2026-11-20-campaign-a-library-django.brief.md)) | A: library, Django developer, 100k loans | 7e0d1ec | SQLite, PostgreSQL | 8/10 | 1 |
| 2026-11-20 | [campaign B](2026-11-20-campaign-b-saas-billing-pg.md) ([brief](2026-11-20-campaign-b-saas-billing-pg.brief.md)) | B: SaaS billing, backend engineer, 100k invoice lines | 7e0d1ec | PostgreSQL | 7/10 | 0 |
| 2026-11-20 | [campaign C](2026-11-20-campaign-c-event-booking-mysql.md) ([brief](2026-11-20-campaign-c-event-booking-mysql.brief.md)) | C: event booking, indie developer, 100k bookings | 7e0d1ec | MySQL 8.4, MariaDB 11.4, SQLite | 8/10 | 1 |

## Open findings

None is a `BLOCKER` or `DOC-WRONG`. All are planned after 0.2.0.

- Values of the wrong type passed with a string column name are not checked against the schema type (2026-11-08: A #12, B #13, C #13; 2026-11-20: A #4, B #9, C #4). Typed columns catch it at compile time; filter.md documents the per-backend behaviour.
- `group_by` on expressions (2026-11-20 B #2); raw SQL meanwhile.
- `raw` maps rows to models only (2026-11-08 C #18, 2026-11-20 C #1); an unmanaged model is the documented workaround.
- Many-to-many pair insert for seeding pivots (2026-11-20 A #3); raw SQL meanwhile.
- `makemigrations` warning when a partial unique index is skipped on MySQL / MariaDB (2026-11-20 C #6).
- `restore` cannot tell hand-written migration SQL (functions, triggers) from generated files, so it cannot warn that those objects are not recreated (2026-11-20 A #10, C #10); documented in migrate.md.
- No typed error for numeric overflow (2026-11-08 C #20).
- Two slow-query `WARN` events for one statement on MySQL, not reproduced yet (2026-11-08 C #21).

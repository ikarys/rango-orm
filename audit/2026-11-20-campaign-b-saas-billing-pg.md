---
date: 2026-11-20
rango_commit: 7e0d1ec
db: PostgreSQL (shared dev instance, 127.0.0.1:5432, database blind_b)
persona: backend engineer, strong SQL/PostgreSQL, new to Rango
model: Sonnet 5.5
campaign: B (multi-tenant SaaS billing API, PostgreSQL)
---

## Brief

Built a billing API library plus step binaries in /tmp/rango-blind-b/billing: 7 models (tenant, customer, product, invoice, invoice_line, payment, tag M2M), 4 migrations (the 4th hand-written), 142k rows seeded, all 12 steps run against real PostgreSQL.

Blindness: under the Rango repo I read only docs/src/ and README.md, plus cargo doc output of my own app. I never opened source, tests, examples, AGENTS.md, CHANGELOG.md, audit/ or git history, and did not follow repo paths in errors. Two allowed deviations: the CLI was built from the repo with `cargo install --path`, and the library is a path dependency.

## (a) Friction log

| # | Step | Severity | What happened | Expected | Exact error | Suggested fix |
|---|------|----------|---------------|----------|-------------|---------------|
| 1 | 12 export/import | PAINFUL | CSV export writes JSON fields (`{"bulk":true,"plan":"pro"}`) unquoted, so commas split the row; import of own export fails. JSON export/import is fine (2000 rows, `--replace` ok). | CSV quoting per RFC 4180 | `invalid input syntax for type uuid: ""plan":"pro"},df1654f6-be92-42ce-8732-681feb7ca620" at line 138` | Quote fields containing commas/quotes; add a CSV round-trip example in docs |
| 2 | 6 queries | PAINFUL | `group_by` accepts columns only; monthly revenue (`date_trunc`) needs raw SQL. | Expression group-by or documented limit | none (no API) | Document the limit on the group-by page and point to `raw` |
| 3 | 6 queries | PAINFUL | Model `ordering = [desc("id")]` is applied to `aggregate()`/`group_by`, so PG rejects it. Workaround: explicit `.order_by(alias)`. | Default ordering ignored for aggregates | PG `42803` (column "invoice.id" must appear in the GROUP BY clause or be used in an aggregate function) | Drop default ordering for aggregate, or document it |
| 4 | 12 restore | PAINFUL | Restore of 142k rows took 1m2.8s (~2.3k rows/s) vs ~120k rows/s for seed via `bulk_create`. | Comparable to bulk speed | none | Use multi-row/UNNEST inserts in restore; document throughput |
| 5 | 12 security | DOC-WRONG | credentials.md says the DML-only app role cannot dump/backup. With the docs' own default privileges the app role can. | Doc matches behaviour | none | Fix the sentence or state the required grants |
| 6 | 4 evolve | PAINFUL | No documented way to add hand-written migrations (GIN indexes for JSONB/FTS). Works by dropping a numbered .sql file; `makemigrations --check` stays clean; `restore` drops those indexes (warns). | Documented escape hatch | none | Add a "custom SQL in migrations" section and mention restore behaviour |
| 7 | 3 makemigrations | MINOR | `--dry-run` still needs `--yes` when a rename is detected. | Dry-run never prompts | prompt on rename | Skip prompt in dry-run |
| 8 | 6/10 observability | MINOR | `.explain()` returns SQL text only; for partial/aggregate queries it shows `SELECT *`, not the real columns. | Real SQL | n/a | Document, or render real projection |
| 9 | 6 queries | MINOR | `offset()` takes i64 (usize habit); wrong-type string filter value surfaces a raw PG error. | Typed hint | PG type error | Mention in the filter docs |
| 10 | 2 models | MINOR | `#[field(default = ...)]` still requires the field in struct literals; decimals silently rounded, sums come back f64; FK default is cascade (restrict needs `on_delete`). | Documented | none | Doc notes |
| 11 | 10 observability | MINOR | `DatabaseConfig.after_connect` is undocumented, timeout units unclear; BEGIN/COMMIT not visible in `sqlx::query` event counts. | Documented | none | Doc notes |
| 12 | 6 volume | MINOR | `Customer::filter().all()` returned 1000 of 2000 rows silently (cap documented but no warning). | Cap warning/log | none | Emit a tracing warn on cap hit |

## (b) Effort per step

| Step | Rust lines | SQL calls |
|------|-----------|-----------|
| 1 install/init/config | ~15 | 0 |
| 2 models | ~170 | 0 |
| 3 makemigrations+migrate | 0 | ~30 (DDL) |
| 4 evolve schema | ~25 | ~10 |
| 5 seed 142k | ~90 | ~300 bulk statements |
| 6 queries | ~160 | ~40 |
| 7 volume checks (keyset 20002 rows, EXPLAIN) | ~50 | ~45 |
| 8 atomic issue_invoice | ~45 | 6 per invoice |
| 9 concurrency/retry (serializable conflict) | ~40 | ~20 |
| 10 observability (counter Layer) | ~35 | n/a |
| 11 deliberate mistakes | ~60 | ~15 |
| 12 dump/restore/backup | 0 | CLI only |

## (c) What was simple

- Models to migrations to schema: one command each, correct indexes/constraints, partial unique index included.
- Least-privilege roles (`MIGRATE_DATABASE_URL`) worked first try.
- TLS default (require on non-loopback) confirmed.
- `atomic_retry` handled a real serializable conflict.
- Keyset pagination walked 20002 invoices with no gaps or duplicates; indexes used.
- Typed errors clear (TableMissing, did-you-mean on unknown names).
- JSON dump/restore/backup round trip.

## (d) Documentation pages wrong, missing or misleading

- security/credentials.md: wrong claim about app role and dump/backup (finding 5).
- Querying/group-by: does not state the column-only limit nor the default-ordering interaction (2, 3).
- Migrations: no page on hand-written migrations or JSONB/FTS indexes, nor restore's effect on them (6).
- CLI export/import: CSV with JSON columns not covered (1); restore speed not stated (4).
- Config: `after_connect`, timeout units (11).

## (e) Verdict

Score: 7/10. Would use it: yes, with caveats. The model-to-migration loop and safety defaults are excellent. Gaps are at the edges: aggregates, bulk restore speed, CSV, and the missing custom-SQL migration story. No BLOCKER: every step completed, with raw SQL for the monthly revenue report.

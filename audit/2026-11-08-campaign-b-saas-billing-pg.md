# Blind test report, campaign B (SaaS billing, PostgreSQL)

- Date: 2026-11-08
- Rango commit: bd5de84 (0.2.0-beta.1, path dependency)
- Database: PostgreSQL 17 (shared devpg), db `blind_b`, roles `app_migrate` (DDL) and `app` (DML only)
- Persona: backend engineer, strong SQL/PostgreSQL, checks SQL, query counts, transaction semantics
- Tester model: Claude Sonnet 5.5
- App: /tmp/rango-blind-b/billing (lib + `src/bin/run.rs` with subcommands seed, queries, tx, conc, obs, mistakes); scratch log /tmp/rango-blind-b/friction.md

## (a) Friction log

| # | Step | Severity | Finding | Repro / exact text |
|---|------|----------|---------|--------------------|
| 1 | 6 | DOC-WRONG | pagination.md keyset loop: page 1 has no `order_by`. With a model default `ordering = [desc("created_at")]`, page 1 is ordered by created_at and later pages by the keyset column, so rows are skipped silently (walked 609 of 1560). Without default ordering page 1 has no ORDER BY. | `invoice_page(None, 500)` then `.before(NUMBER, last)`; no error. Fix: explicit `.order_by(NUMBER.desc())` on every page. |
| 2 | 10 | DOC-WRONG | raw-sql.md uses unquoted `FROM user` / `UPDATE user`. On PostgreSQL `user` is the current_user function: the query runs and returns the wrong thing, no error. | `select count(*) from user` returns `1`. |
| 3 | 4 | PAINFUL | Editing the SQL text of a CheckConstraint but keeping its name: makemigrations says nothing to do. Silent drift between models and DB. | Change `status IN (...)` in `chk_invoice_status`, run `rango makemigrations`: `✅ Nothing to migrate — models match the snapshot.` Renaming the constraint works (ADD then DROP). |
| 4 | 8 | PAINFUL | `atomic_retry` future is not Send-general in `tokio::spawn`; docs promise Send for `atomic()` and show spawn. A retrying payment worker is the headline use case. | `tokio::spawn(async move { atomic_retry(&pool, 6, async \|tx\| {..}).await })`: `error: implementation of `Send` is not general enough ... this async fn captures a value whose type is not `Send``. Workaround: `tokio::join!` or `atomic()` plus own retry loop. |
| 5 | 6 | PAINFUL | `.all()` returns `Vec<WithRelated<M>>` with only Deref, no `into_inner`; models are not Clone by default, docs never say they may derive Clone/Debug. | I had to `#[derive(Model, Clone, Debug)]` and write `(*w).clone()`. |
| 6 | 10 | PAINFUL | Docs say string column names are validated into `Error::Invalid`, but `bulk_upsert` conflict columns and `get_or_create` lookups are not: raw PG error. | `bulk_upsert(&pool, &[tag], &["bogus"])`: `Database { code: Some("42703"), message: "column \"bogus\" does not exist" }`; same for `get_or_create(.., vec![("bogus", ..)], ..)`. |
| 7 | 9 | MINOR | `select_as::<P>().all()` emits no `db.query` span (insert, get, update, filter.all, values, count, bulk_create, delete, raw all emit 1). tracing.md says every operation does. | Counter in src/counter.rs: `[filter.select_as.all] sqlx statements: 1, db.query spans: 0`. |
| 8 | 9 | MINOR | No user-guide page documents TLS (`sslmode=verify-full`, remote default `require`); only the trust/security page does. credentials.md and backends/postgres.md are silent. | grep docs/src outside trust/. |
| 9 | 1 | MINOR | credentials.md roles SQL works but does not say to run it inside the target database (schema grants are per database), omits CONNECT, and a restore target needs the same grant again. | Restore into fresh db as app_migrate: `permission denied for schema public at line 2843`. |
| 10 | 3 | MINOR | Docs name migration files `NNNN_<tables>.sql`; with 8 tables the file is `0001_auto.sql`. Fallback undocumented. | `rango makemigrations` with 8 tables. |
| 11 | 2 | MINOR | `FieldVarchar::new(..)?` in a fn returning `rango::Result` fails: `the trait From<FieldError> is not implemented for rango::Error`. | Write a lib fn returning `rango::Result`. |
| 12 | 6 | MINOR | Default 1000-row cap is hit silently (`unpaid_invoices` returned exactly 1000 of 1560). Documented, but nothing signals truncation. | `.all()` on a 1560-row match. |
| 13 | 10 | MINOR | Wrong-type value gives raw PG text, no typed error, no column named. | `eq("total", "abc")`: `Database { code: Some("42883"), message: "operator does not exist: numeric = text" }`; `get::<_, Tenant>(.., Text)`: `operator does not exist: uuid = text`. |
| 14 | 6 | MINOR | `sum()` returns f64 (572629.8) on a decimal column; exact money sum needs `raw_scalar::<_, Decimal>` (572629.80). Query builder has no GROUP BY and `raw` is model-only, so revenue per tenant per month needs raw SQL returning jsonb via `raw_scalar::<_, serde_json::Value>`. | Dashboard query in src/queries.rs. |
| 15 | 11 | MINOR | `dump`/`backup` need a role that can read every table: DML-only `app` fails with `permission denied for table _rango_migrations`. `dump --schema-only` prints `Dumped 0 table(s)`. Restore errors mention a meaningless `at line 2843`. Re-restore into a non-empty db: `duplicate key value violates unique constraint "_rango_migrations_name_key" at line 666`. | `rango dump -d postgres://app:app@...`. |
| 16 | 10 | MINOR | `update_or_create` overwrites the whole row from the model values (customer metadata reset to `{}`); PK is kept. Not stated clearly. | `update_or_create` on an existing customer with new model value. |
| 17 | 2 | MINOR | `serde_json` is needed for FieldJson and raw jsonb results but not listed in the install docs. | Cargo.toml. |

No BLOCKER. No source was read.

## (b) Per step: lines of Rust and SQL statements

| Step | Rust lines | SQL statements (from sqlx::query events) |
|------|-----------|-------------------------------------------|
| 1-2 models | models.rs 93 | 0 |
| 3-4 migrations | 0 Rust, 102 lines generated SQL (3 files) | n/a (CLI) |
| 5 seed | seed.rs 79 | 3600 customers + 150 products etc: 12 statements; `M2M::set` for 60 products 268; bulk_create 1500 invoices 1 (UNNEST); bulk_update 1000 invoices 1 (UPDATE FROM VALUES); bulk_upsert 150 customers 1 |
| 6 queries | queries.rs 62 | keyset page 1; select_related 1; prefetch_related 2 (`= ANY($1)`); M2M prefetch 2; not+where_exists 1; in_subquery+json 1; explain_analyze showed index `uniq_invoice_number` |
| 7 atomicity | ops.rs 107 (all of steps 7-8) | issue_invoice 6 per invoice; failure (missing product / customer) rolls back, counter advanced exactly once |
| 8 concurrency | (in ops.rs) | serializable: 3 attempts incl. one real 40001, no double pay (179.94 of 179.94); FOR UPDATE: 8 workers, 1 paid, 7 aborted |
| 9 observability | counter.rs 53 | slow-query WARN fired at 20 ms threshold; statement_timeout 100 ms gave `Timeout` |
| 10 mistakes | run.rs 275 total | n/a |
| 11 dump/restore | 0 | restore of 5894 rows into empty db, per-table md5 identical, same 26 indexes and 30 constraints |

## (c) What felt simple

- Typed errors are good: TableMissing, UniqueViolation/CheckViolation/ForeignKeyViolation all name the constraint (`uniq_one_draft`, `chk_invoice_status_v2`).
- Bulk ops are a single statement; SQL is as a PG engineer would write it; the 1000-row cap and `.explain()` make behaviour inspectable.
- Rename detection with data preserved (`--yes` for non-TTY, with a clear message); `did you mean` hints for string columns.
- Roles split via MIGRATE_DATABASE_URL works; app role cannot do DDL.
- dump/restore is exact, backup file is 0600.

## (d) Doc pages wrong, missing or misleading

- querying/pagination.md: first page without order_by (finding 1).
- raw-sql.md: unquoted `user` (2).
- transactions.md: no mention that atomic_retry is not spawnable (4); models: Clone/Debug and `WithRelated` ownership (5).
- makemigrations.md: constraint text edits not detected, file naming fallback (3, 10).
- Missing: TLS user page (8), roles/dump privileges, restore target grants (9, 15), GROUP BY / revenue-style reports (14).
- tracing.md: select_as has no span (7); errors page: string-name validation coverage (6).

## (e) Verdict

Score: 7/10. For this app I would use Rango for the CRUD, bulk, keyset, relations and transaction parts, which behave as a SQL-literate engineer expects. I would not trust it yet for schema evolution of constraints (finding 3) or a spawned retrying worker (4); revenue reporting needs raw SQL.

Counts: BLOCKER 0, PAINFUL 4, DOC-WRONG 2, MINOR 11.

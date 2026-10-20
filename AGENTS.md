# Orbit agent instructions

Read [`.ai/README.md`](.ai/README.md) first: it lists the project guides in order.

## Databases: SQLite and Postgres

Orbit runs on SQLite (default, `ORBIT_DATABASE_PATH`) or Postgres (`ORBIT_DATABASE_URL`).
Every change must work on both.

- **SQL:** all queries use the sqlx `Any` driver. Write placeholders as `$1, $2, …` (never `?`). Build dynamic SQL
  with `orbit_platform::SqlBuilder`, not `sqlx::QueryBuilder`.
- **Rows and types:** import `orbit_platform::Row`, not `sqlx::Row`. Flags are 0/1 `BIGINT` columns: bind `i64`
  (never `bool`, `u32` or `u64`), and decode boolean results such as `EXISTS (…)` as `orbit_platform::Flag`.
- **Portable SQL:** use `ON CONFLICT` (not `INSERT OR IGNORE`), `IS [NOT] DISTINCT FROM` (not `IS $n`),
  `CAST(SUM(…) AS BIGINT)`, and `lower(…)` for case-insensitive text. When SQL must differ (search, JSON, time),
  branch on `database.backend()` (`Backend::Sqlite` / `Backend::Postgres`).
- **Transactions:** writes use `immediate_transaction()`. It serializes writers on both databases. On Postgres, an
  error ends the whole transaction, so do not catch a database error and continue in the same transaction.
- **Migrations:** a new SQLite migration in `apps/server/migrations/` also needs the same change in the Postgres
  schema in `apps/server/migrations/postgres/` (see `.ai/lessons.md` for the other places to update).
- **Tests:** run the tests that cover your change on both databases:
  `cargo test -p orbit-server --test <name>`, then the same with
  `ORBIT_TEST_POSTGRES_URL=postgres://orbit:orbit@127.0.0.1:15432/orbit`. Tests that only make sense on SQLite
  start with `if orbit_platform::testing_postgres() { return; }` and a one-line reason.
- **Postgres setup:** the database must use `LOCALE_PROVIDER builtin BUILTIN_LOCALE 'C.UTF-8' LOCALE 'C.UTF-8'`
  (Orbit refuses other databases), and the server needs `ORBIT_APP_KEY`. S3 for files and backups stays optional
  and is set in Admin → Storage. Operator details: [`docs/operations.md`](docs/operations.md).

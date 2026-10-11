use std::fmt::{Display, Write};

use sqlx::any::AnyArguments;
use sqlx::query::Query;
use sqlx::{Any, Arguments, Encode, SqlSafeStr, SqlStr, Type};

/// Builds a query with bound values for both backends, like `sqlx::QueryBuilder`, but writes the
/// placeholders as `$1, $2, …`: SQLite binds them by number too, while the Any driver's own
/// builder always writes `?`, which Postgres rejects.
#[derive(Default)]
pub struct SqlBuilder {
    sql: String,
    arguments: AnyArguments,
}

impl SqlBuilder {
    #[must_use]
    pub fn new(init: impl Into<String>) -> Self {
        Self {
            sql: init.into(),
            ..Self::default()
        }
    }

    /// Appends trusted SQL text.
    pub fn push(&mut self, sql: impl Display) -> &mut Self {
        write!(self.sql, "{sql}").expect("writing to a String cannot fail");
        self
    }

    /// Appends a placeholder bound to `value`.
    pub fn push_bind<'t, T>(&mut self, value: T) -> &mut Self
    where
        T: Encode<'t, Any> + Type<Any>,
    {
        // The Any driver's encoders only copy the value into the buffer.
        self.arguments
            .add(value)
            .expect("Any arguments accept every Any-encodable value");
        let number = self.arguments.len();
        self.push(format_args!("${number}"))
    }

    /// Appends items with `separator` between them, e.g. the values of an `IN (…)` list.
    pub fn separated<S: Display>(&mut self, separator: S) -> Separated<'_, S> {
        Separated {
            builder: self,
            separator,
            first: true,
        }
    }

    #[must_use]
    pub fn sql(&self) -> SqlStr {
        sqlx::AssertSqlSafe(self.sql.clone()).into_sql_str()
    }

    /// The query with the bound values; the builder is left with none.
    pub fn build(&mut self) -> Query<'_, Any, AnyArguments> {
        sqlx::query_with(self.sql(), std::mem::take(&mut self.arguments))
    }
}

/// See [`SqlBuilder::separated`].
pub struct Separated<'b, S> {
    builder: &'b mut SqlBuilder,
    separator: S,
    first: bool,
}

impl<S: Display> Separated<'_, S> {
    fn separate(&mut self) {
        if !self.first {
            self.builder.push(&self.separator);
        }
        self.first = false;
    }

    /// Appends trusted SQL text after a separator.
    pub fn push(&mut self, sql: impl Display) -> &mut Self {
        self.separate();
        self.builder.push(sql);
        self
    }

    /// Appends trusted SQL text without a separator.
    pub fn push_unseparated(&mut self, sql: impl Display) -> &mut Self {
        self.builder.push(sql);
        self
    }

    /// Appends a bound placeholder after a separator.
    pub fn push_bind<'t, T>(&mut self, value: T) -> &mut Self
    where
        T: Encode<'t, Any> + Type<Any>,
    {
        self.separate();
        self.builder.push_bind(value);
        self
    }
}

#[cfg(test)]
mod tests {
    use sqlx::Row;

    use super::SqlBuilder;
    use crate::TestDatabase;

    #[test]
    fn numbers_placeholders_in_order() {
        let mut query = SqlBuilder::new("SELECT id FROM tasks WHERE workspace_id = ");
        query.push_bind("workspace").push(" AND id IN (");
        let mut ids = query.separated(", ");
        ids.push_unseparated("");
        for id in ["a", "b", "c"] {
            ids.push_bind(id);
        }
        query.push(") LIMIT ").push_bind(10_i64);

        assert_eq!(
            query.sql().as_str(),
            "SELECT id FROM tasks WHERE workspace_id = $1 AND id IN ($2, $3, $4) LIMIT $5"
        );
    }

    #[tokio::test]
    async fn runs_on_the_test_database() {
        let database = TestDatabase::new().await.unwrap();
        let mut query = SqlBuilder::new("SELECT ");
        query
            .push_bind(40_i64)
            .push(" + ")
            .push_bind(2_i64)
            .push(" AS answer, ")
            .push_bind("text")
            .push(" AS label");
        let row = query.build().fetch_one(database.pool()).await.unwrap();
        assert_eq!(row.get::<i64, _>("answer"), 42);
        assert_eq!(row.get::<String, _>("label"), "text");

        let mut query =
            SqlBuilder::new("SELECT COUNT(*) FROM schema_migrations WHERE version IN (");
        let mut versions = query.separated(", ");
        for version in [1_i64, 46, 999] {
            versions.push_bind(version);
        }
        query.push(")");
        let row = query.build().fetch_one(database.pool()).await.unwrap();
        assert!(row.get::<i64, _>(0) >= 1);
    }
}

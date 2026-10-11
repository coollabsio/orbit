use sqlx::any::{AnyRow, AnyTypeInfo, AnyTypeInfoKind, AnyValueRef};
use sqlx::error::BoxDynError;
use sqlx::{Any, ColumnIndex, Decode, Type, ValueRef};

/// A boolean column: Orbit stores flags as 0/1 integers and Postgres expressions such as
/// `EXISTS (...)` return a real boolean, but the Any driver decodes `bool` only from the latter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Flag(pub bool);

impl Type<Any> for Flag {
    fn type_info() -> AnyTypeInfo {
        <i64 as Type<Any>>::type_info()
    }

    fn compatible(ty: &AnyTypeInfo) -> bool {
        ty.kind() == AnyTypeInfoKind::Bool || ty.kind().is_integer()
    }
}

impl<'r> Decode<'r, Any> for Flag {
    fn decode(value: AnyValueRef<'r>) -> Result<Self, BoxDynError> {
        if value.type_info().kind() == AnyTypeInfoKind::Bool {
            <bool as Decode<Any>>::decode(value).map(Flag)
        } else {
            <i64 as Decode<Any>>::decode(value).map(|value| Flag(value != 0))
        }
    }
}

/// `sqlx::Row` for Orbit rows: the same `get`/`try_get`, but `bool` columns decode on both
/// backends (through [`Flag`]). Import it instead of `sqlx::Row`.
pub trait Row {
    /// # Panics
    /// If the column is missing or does not decode as `T`, like `sqlx::Row::get`.
    fn get<'r, T: Column<'r>, I: ColumnIndex<AnyRow>>(&'r self, index: I) -> T;

    fn try_get<'r, T: Column<'r>, I: ColumnIndex<AnyRow>>(
        &'r self,
        index: I,
    ) -> Result<T, sqlx::Error>;
}

impl Row for AnyRow {
    fn get<'r, T: Column<'r>, I: ColumnIndex<AnyRow>>(&'r self, index: I) -> T {
        self.try_get(index)
            .unwrap_or_else(|error| panic!("{error}"))
    }

    fn try_get<'r, T: Column<'r>, I: ColumnIndex<AnyRow>>(
        &'r self,
        index: I,
    ) -> Result<T, sqlx::Error> {
        T::read(self, index)
    }
}

/// A Rust type that [`Row`] can read from a column.
pub trait Column<'r>: Sized {
    fn read<I: ColumnIndex<AnyRow>>(row: &'r AnyRow, index: I) -> Result<Self, sqlx::Error>;
}

impl<'r, T: Decode<'r, Any> + Type<Any>> Column<'r> for T
where
    T: NotBool,
{
    fn read<I: ColumnIndex<AnyRow>>(row: &'r AnyRow, index: I) -> Result<Self, sqlx::Error> {
        sqlx::Row::try_get(row, index)
    }
}

impl<'r> Column<'r> for bool {
    fn read<I: ColumnIndex<AnyRow>>(row: &'r AnyRow, index: I) -> Result<Self, sqlx::Error> {
        sqlx::Row::try_get::<Flag, I>(row, index).map(|flag| flag.0)
    }
}

impl<'r> Column<'r> for Option<bool> {
    fn read<I: ColumnIndex<AnyRow>>(row: &'r AnyRow, index: I) -> Result<Self, sqlx::Error> {
        sqlx::Row::try_get::<Option<Flag>, I>(row, index).map(|flag| flag.map(|flag| flag.0))
    }
}

/// Every column type except `bool` and `Option<bool>`, which [`Row`] reads through [`Flag`].
pub trait NotBool {}

macro_rules! not_bool {
    ($($type:ty),* $(,)?) => {
        $(
            impl NotBool for $type {}
            impl NotBool for Option<$type> {}
        )*
    };
}

not_bool!(i16, i32, i64, f32, f64, String, Vec<u8>, Flag);
impl NotBool for &str {}
impl NotBool for Option<&str> {}
impl NotBool for &[u8] {}
impl NotBool for Option<&[u8]> {}

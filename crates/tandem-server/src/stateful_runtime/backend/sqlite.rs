//! SQLite arm of the storage backend facade. This is a thin adapter over
//! rusqlite: statements pass through untranslated, so behavior is identical
//! to the pre-facade store.

use rusqlite::types::{ToSqlOutput, ValueRef};

use super::{Error, Result, Row, Value};

impl rusqlite::ToSql for Value {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(match self {
            Value::Null => ToSqlOutput::Borrowed(ValueRef::Null),
            Value::Integer(value) => ToSqlOutput::Borrowed(ValueRef::Integer(*value)),
            Value::Real(value) => ToSqlOutput::Borrowed(ValueRef::Real(*value)),
            Value::Text(value) => ToSqlOutput::Borrowed(ValueRef::Text(value.as_bytes())),
            Value::Blob(value) => ToSqlOutput::Borrowed(ValueRef::Blob(value)),
            Value::OutOfRange(original) => {
                return Err(rusqlite::Error::ToSqlConversionFailure(
                    format!("integer parameter {original} is out of the signed 64-bit range")
                        .into(),
                ))
            }
        })
    }
}

fn value_from_ref(value: ValueRef<'_>) -> Result<Value> {
    Ok(match value {
        ValueRef::Null => Value::Null,
        ValueRef::Integer(value) => Value::Integer(value),
        ValueRef::Real(value) => Value::Real(value),
        ValueRef::Text(value) => Value::Text(
            std::str::from_utf8(value)
                .map_err(|error| Error::Conversion(format!("non-UTF-8 text column: {error}")))?
                .to_string(),
        ),
        ValueRef::Blob(value) => Value::Blob(value.to_vec()),
    })
}

pub(super) fn execute(
    connection: &rusqlite::Connection,
    sql: &str,
    params: &[Value],
) -> Result<usize> {
    let mut statement = connection.prepare(sql)?;
    statement
        .execute(rusqlite::params_from_iter(params.iter()))
        .map_err(Error::from)
}

pub(super) fn query(
    connection: &rusqlite::Connection,
    sql: &str,
    params: &[Value],
) -> Result<Vec<Row>> {
    let mut statement = connection.prepare(sql)?;
    let column_count = statement.column_count();
    let mut rows = statement.query(rusqlite::params_from_iter(params.iter()))?;
    let mut output = Vec::new();
    while let Some(row) = rows.next()? {
        let mut values = Vec::with_capacity(column_count);
        for index in 0..column_count {
            values.push(value_from_ref(row.get_ref(index)?)?);
        }
        output.push(Row::new(values));
    }
    Ok(output)
}

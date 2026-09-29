use super::StoreError;
use diesel::{prelude::*, sql_types::Text, sqlite::Sqlite};
use serde::{Serialize, de::DeserializeOwned};
#[derive(QueryableByName)]
struct JsonRow {
    #[diesel(sql_type = Text)]
    data: String,
}
pub(super) fn json(value: &impl Serialize) -> Result<String, StoreError> {
    Ok(serde_json::to_string(value)?)
}
pub(super) fn rows<T: DeserializeOwned>(
    c: &mut SqliteConnection,
    sql: &str,
    params: &[&str],
) -> Result<Vec<T>, StoreError> {
    let mut query = diesel::sql_query(sql).into_boxed::<Sqlite>();
    for param in params {
        query = query.bind::<Text, _>(*param);
    }
    query
        .load::<JsonRow>(c)?
        .into_iter()
        .map(|r| serde_json::from_str(&r.data).map_err(StoreError::from))
        .collect()
}
pub(super) fn execute(
    c: &mut SqliteConnection,
    sql: &str,
    params: &[&str],
) -> Result<usize, StoreError> {
    let mut query = diesel::sql_query(sql).into_boxed::<Sqlite>();
    for param in params {
        query = query.bind::<Text, _>(*param);
    }
    Ok(query.execute(c)?)
}

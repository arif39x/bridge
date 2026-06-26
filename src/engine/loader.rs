use crate::engine::db::{generic_select_in, validate_identifier};
use crate::error::BridgeResult;
use sqlx::{any::AnyRow, Row, AnyPool};
use std::collections::HashMap;
use uuid::Uuid;

pub async fn batch_load(
    pool: &AnyPool,
    parent_ids: &[Uuid],
    child_table: &str,
    foreign_key: &str,
    url: &str,
) -> BridgeResult<HashMap<Uuid, Vec<AnyRow>>> {
    validate_identifier(child_table)?;
    validate_identifier(foreign_key)?;

    if parent_ids.is_empty() {
        return Ok(HashMap::new());
    }

    let parent_id_strs: Vec<String> = parent_ids.iter().map(|id| id.to_string()).collect();
    let rows = generic_select_in(pool, None, url, child_table, foreign_key, &parent_id_strs).await?;

    let mut grouped: HashMap<Uuid, Vec<AnyRow>> = HashMap::new();
    for row in &rows {
        if let Ok(fk) = row.try_get::<String, _>(foreign_key) {
            if let Ok(uuid) = Uuid::parse_str(&fk) {
                grouped.entry(uuid).or_default().push(row.clone());
            }
        }
    }
    Ok(grouped)
}

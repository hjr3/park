use anyhow::Result;
use sqlx::sqlite::SqlitePool;
use uuid::Uuid;

use crate::har::Har;

pub async fn insert_request(pool: &SqlitePool, har: &Har) -> Result<Uuid> {
    tracing::trace!("insert_request");
    let mut conn = pool.acquire().await?;

    let query = r#"
        INSERT INTO requests
        (
            request_id,
            har,
            created_at
        )
        VALUES (
            ?,
            jsonb(?),
            strftime('%s','now')
        )
    "#;

    let request_id = Uuid::now_v7();
    let har_json = serde_json::to_string(har).map_err(|err| {
        tracing::error!("Failed to serialize HAR to JSON");
        err
    })?;

    sqlx::query(query)
        .bind(&request_id.to_string())
        .bind(&har_json)
        .execute(&mut *conn)
        .await
        .map_err(|err| {
            tracing::error!("Failed to save request. {:?}", &har);
            err
        })?;

    Ok(request_id)
}

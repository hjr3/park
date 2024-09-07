use anyhow::Result;
use sqlx::sqlite::SqlitePool;
use uuid::Uuid;

use crate::config;
use crate::har::Har;

pub async fn init_db(db_config: &config::Database) -> Result<SqlitePool> {
    tracing::trace!("init_db");
    let pool = SqlitePool::connect(&db_config.uri).await?;

    // TODO only run migrations if the database is empty
    sqlx::migrate!().run(&pool).await.map_err(|err| {
        tracing::error!("Failed to run migrations");
        err
    })?;

    let max_size = db_config.max_size;
    let pool2 = pool.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;
            let size = sqlx::query_scalar(
                "SELECT page_count * page_size FROM pragma_page_count(), pragma_page_size()",
            )
            .fetch_one(&pool2)
            .await
            .unwrap_or(0u64);

            if size > max_size {
                sqlx::query("VACUUM").execute(&pool2).await.unwrap();
            }
        }
    });

    Ok(pool)
}

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

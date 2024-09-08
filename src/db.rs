use anyhow::Result;
use sqlx::sqlite::SqlitePool;
use sqlx::QueryBuilder;
use sqlx::Row;
use uuid::Uuid;

use crate::config;
use crate::har::Har;

pub async fn init_db(db_config: &config::Database) -> Result<SqlitePool> {
    tracing::trace!("init_db");
    let pool = SqlitePool::connect(&db_config.uri).await?;

    let user_tables: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'",
    )
    .fetch_one(&pool)
    .await?;

    if user_tables.0 == 0 {
        tracing::info!("Running migrations...");
        sqlx::migrate!().run(&pool).await.map_err(|err| {
            tracing::error!("Failed to run migrations");
            err
        })?;
    } else {
        tracing::info!("Existing tables found, skipping migrations");
    }

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

pub async fn insert_request(pool: &SqlitePool, har: &mut Vec<Har>) -> Result<()> {
    tracing::trace!("insert_request");
    let mut conn = pool.acquire().await?;

    let mut query = QueryBuilder::new(
        r#"
        INSERT INTO requests
        (
            request_id,
            har,
            created_at
        )"#,
    );

    let iter = har
        .drain(..)
        .map(|har| {
            let request_id = Uuid::now_v7();
            match serde_json::to_string(&har) {
                Ok(har_json) => Ok((request_id, har_json)),
                Err(err) => {
                    tracing::error!("Failed to serialize HAR to JSON");
                    Err(err)
                }
            }
        })
        .filter_map(Result::ok);

    query.push_values(iter, |mut b, (request_id, har_json)| {
        b.push_bind(request_id.to_string())
            .push_bind(har_json)
            .push("strftime('%s','now')");
    });

    let query = query.build();
    query.execute(&mut *conn).await?;

    Ok(())
}

pub async fn latest_request(pool: &SqlitePool) -> Result<Option<Har>> {
    tracing::trace!("latest_request");
    let mut conn = pool.acquire().await?;

    let query = r#"
        SELECT json(har)
        FROM requests
        ORDER BY request_id DESC
        LIMIT 1
    "#;

    let row = sqlx::query(query)
        .fetch_optional(&mut *conn)
        .await
        .map_err(|err| {
            tracing::error!("Failed to fetch latest request");
            err
        })?;

    let har_json: Option<Vec<u8>> = match row {
        Some(row) => row.get(0),
        None => return Ok(None),
    };

    if let Some(har_json) = har_json {
        let har: Har = serde_json::from_slice(&har_json).map_err(|err| {
            tracing::error!("Failed to deserialize HAR from JSON");
            err
        })?;

        Ok(Some(har))
    } else {
        Ok(None)
    }
}

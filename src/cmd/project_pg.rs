use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use url::Url;
use zamsync_core::Event;

pub fn run(
    url: &str,
    events: impl Iterator<Item = Result<Event, Box<dyn std::error::Error>>>,
    batch_size: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    rt.block_on(run_async(url, events, batch_size))
}

async fn ensure_database(url: &str) -> Result<(), Box<dyn std::error::Error>> {
    let parsed = Url::parse(url)?;
    let db_name = parsed.path().trim_start_matches('/').to_string();
    if db_name.is_empty() {
        return Err("PostgreSQL URL must include a database name".into());
    }

    let mut maintenance_url = parsed.clone();
    maintenance_url.set_path("/postgres");
    let maint_pool = PgPoolOptions::new()
        .max_connections(1)
        .connect(maintenance_url.as_str())
        .await?;

    let exists: (bool,) =
        sqlx::query_as("SELECT EXISTS(SELECT 1 FROM pg_database WHERE datname = $1)")
            .bind(&db_name)
            .fetch_one(&maint_pool)
            .await?;

    if !exists.0 {
        // CREATE DATABASE cannot run inside a transaction
        let stmt = format!("CREATE DATABASE \"{}\"", db_name.replace('"', "\"\""));
        sqlx::query(&stmt).execute(&maint_pool).await?;
        eprintln!("created database \"{db_name}\"");
    }

    maint_pool.close().await;
    Ok(())
}

async fn run_async(
    url: &str,
    events: impl Iterator<Item = Result<Event, Box<dyn std::error::Error>>>,
    batch_size: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    ensure_database(url).await?;

    let pool = PgPoolOptions::new().max_connections(4).connect(url).await?;

    init_schema(&pool).await?;

    let mut projected = 0usize;
    let mut skipped = 0usize;
    let mut batch: Vec<Event> = Vec::with_capacity(batch_size);

    for result in events {
        let event = result?;
        batch.push(event);
        if batch.len() >= batch_size {
            let (p, s) = flush_batch(&pool, &batch).await?;
            projected += p;
            skipped += s;
            batch.clear();
        }
    }

    if !batch.is_empty() {
        let (p, s) = flush_batch(&pool, &batch).await?;
        projected += p;
        skipped += s;
    }

    println!("{projected} projected, {skipped} already present");
    Ok(())
}

async fn init_schema(pool: &PgPool) -> Result<(), sqlx::Error> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS zamsync_events (
            id              BIGSERIAL PRIMARY KEY,
            origin_node_id  BIGINT NOT NULL,
            seq             BIGINT NOT NULL,
            hlc_ms          BIGINT NOT NULL,
            hlc_logical     BIGINT NOT NULL,
            event_type      INT NOT NULL,
            payload         BYTEA NOT NULL,
            projected_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            UNIQUE(origin_node_id, seq)
        )",
    )
    .execute(pool)
    .await?;

    sqlx::query("CREATE INDEX IF NOT EXISTS idx_origin_seq ON zamsync_events(origin_node_id, seq)")
        .execute(pool)
        .await?;

    sqlx::query("CREATE INDEX IF NOT EXISTS idx_hlc ON zamsync_events(hlc_ms, hlc_logical)")
        .execute(pool)
        .await?;

    Ok(())
}

async fn flush_batch(
    pool: &PgPool,
    events: &[Event],
) -> Result<(usize, usize), Box<dyn std::error::Error>> {
    let mut projected = 0usize;
    let mut skipped = 0usize;

    let mut tx = pool.begin().await?;

    for ev in events {
        let result = sqlx::query(
            "INSERT INTO zamsync_events \
             (origin_node_id, seq, hlc_ms, hlc_logical, event_type, payload) \
             VALUES ($1, $2, $3, $4, $5, $6) \
             ON CONFLICT DO NOTHING",
        )
        .bind(ev.origin_node.0 as i64)
        .bind(ev.seq.0 as i64)
        .bind(ev.hlc.physical as i64)
        .bind(ev.hlc.logical as i64)
        .bind(ev.event_type as i32)
        .bind(&ev.payload)
        .execute(&mut *tx)
        .await?;

        if result.rows_affected() > 0 {
            projected += 1;
        } else {
            skipped += 1;
        }
    }

    tx.commit().await?;
    Ok((projected, skipped))
}

#[cfg(test)]
mod tests {
    use super::*;
    use zamsync_core::{Hlc, NodeId, SequenceNumber};

    fn pg_url() -> Option<String> {
        std::env::var("TEST_PG_URL").ok()
    }

    use std::sync::OnceLock;
    use tokio::sync::Mutex;

    static TEST_MUTEX: OnceLock<Mutex<()>> = OnceLock::new();

    fn test_mutex() -> &'static Mutex<()> {
        TEST_MUTEX.get_or_init(|| Mutex::new(()))
    }

    async fn setup_pool(url: &str) -> (PgPool, tokio::sync::MutexGuard<'static, ()>) {
        let guard = test_mutex().lock().await;
        let pool = PgPoolOptions::new()
            .max_connections(2)
            .connect(url)
            .await
            .expect("failed to connect to TEST_PG_URL");
        sqlx::query("DROP TABLE IF EXISTS zamsync_events")
            .execute(&pool)
            .await
            .unwrap();
        init_schema(&pool).await.unwrap();
        (pool, guard)
    }

    fn sample_events() -> Vec<Event> {
        vec![
            Event {
                origin_node: NodeId(1),
                seq: SequenceNumber(0),
                hlc: Hlc::new(1000, 0),
                event_type: 1,
                payload: b"hello".to_vec(),
            },
            Event {
                origin_node: NodeId(1),
                seq: SequenceNumber(1),
                hlc: Hlc::new(1001, 0),
                event_type: 1,
                payload: b"world".to_vec(),
            },
        ]
    }

    #[test]
    #[ignore]
    fn test_pg_init_schema() {
        let url = pg_url().expect("TEST_PG_URL required");
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let (pool, _guard) = setup_pool(&url).await;
            // setup_pool already called init_schema once; call again to verify idempotency
            init_schema(&pool).await.unwrap();

            let row: (i64,) =
                sqlx::query_as("SELECT COUNT(*) FROM information_schema.tables WHERE table_name = 'zamsync_events'")
                    .fetch_one(&pool)
                    .await
                    .unwrap();
            assert_eq!(row.0, 1);
        });
    }

    #[test]
    #[ignore]
    fn test_pg_flush_batch_inserts_and_skips() {
        let url = pg_url().expect("TEST_PG_URL required");
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let (pool, _guard) = setup_pool(&url).await;

            let events = sample_events();

            let (p, s) = flush_batch(&pool, &events).await.unwrap();
            assert_eq!(p, 2);
            assert_eq!(s, 0);

            let (p2, s2) = flush_batch(&pool, &events).await.unwrap();
            assert_eq!(p2, 0);
            assert_eq!(s2, 2);
        });
    }
}

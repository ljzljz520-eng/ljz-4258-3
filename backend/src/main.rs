use pv_backend::{api, ingest};
use sqlx::postgres::PgPoolOptions;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| "info,pv_backend=info".into()),
        )
        .init();

    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://pv:pv@localhost:5432/pasteurization".into());
    let pool = PgPoolOptions::new()
        .max_connections(8)
        .connect(&database_url)
        .await?;
    sqlx::migrate!("./migrations").run(&pool).await?;
    tracing::info!("数据库迁移完成");

    // 遥测接入(只读):模拟源或 OPC UA(需 --features opcua-live)
    let ingest_pool = pool.clone();
    tokio::spawn(async move { ingest::run(ingest_pool).await });

    let app = api::router(api::AppState { pool });
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await?;
    tracing::info!("HTTP 监听 :3000");
    axum::serve(listener, app).await?;
    Ok(())
}

//! 数据访问:遥测镜像写入(只追加)、批次/事件/冻结配置/通知/发现。
use crate::domain::*;
use crate::ingest::Reading;
use chrono::{DateTime, Utc};
use sqlx::{PgPool, Row};

// ---------- 遥测镜像 ----------

pub async fn insert_reading(pool: &PgPool, r: &Reading) -> Result<(), sqlx::Error> {
    match r {
        Reading::Temperature(s) => {
            sqlx::query(
                "INSERT INTO telemetry_temperature (device_time, ingest_time, sensor_id, value_c) VALUES ($1,$2,$3,$4)",
            )
            .bind(s.device_time)
            .bind(s.ingest_time)
            .bind(&s.source)
            .bind(s.value)
            .execute(pool)
            .await?;
        }
        Reading::Flow(s) => {
            sqlx::query(
                "INSERT INTO telemetry_flow (device_time, ingest_time, meter_id, value_lph) VALUES ($1,$2,$3,$4)",
            )
            .bind(s.device_time)
            .bind(s.ingest_time)
            .bind(&s.source)
            .bind(s.value)
            .execute(pool)
            .await?;
        }
        Reading::Divert(s) => {
            sqlx::query(
                "INSERT INTO telemetry_divert (device_time, ingest_time, valve_id, position) VALUES ($1,$2,$3,$4)",
            )
            .bind(s.device_time)
            .bind(s.ingest_time)
            .bind(&s.source)
            .bind(s.value.as_str())
            .execute(pool)
            .await?;
        }
    }
    Ok(())
}

#[derive(sqlx::FromRow)]
struct TempRow {
    device_time: DateTime<Utc>,
    ingest_time: DateTime<Utc>,
    sensor_id: String,
    value_c: f64,
}
#[derive(sqlx::FromRow)]
struct FlowRow {
    device_time: DateTime<Utc>,
    ingest_time: DateTime<Utc>,
    meter_id: String,
    value_lph: f64,
}
#[derive(sqlx::FromRow)]
struct DivertRow {
    device_time: DateTime<Utc>,
    ingest_time: DateTime<Utc>,
    valve_id: String,
    position: String,
}

pub async fn telemetry_window(
    pool: &PgPool,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
) -> Result<
    (
        Vec<Sample<f64>>,
        Vec<Sample<f64>>,
        Vec<Sample<DivertPosition>>,
    ),
    sqlx::Error,
> {
    let temps = sqlx::query_as::<_, TempRow>(
        "SELECT device_time, ingest_time, sensor_id, value_c FROM telemetry_temperature WHERE device_time BETWEEN $1 AND $2 ORDER BY device_time",
    )
    .bind(start)
    .bind(end)
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|r| Sample { device_time: r.device_time, ingest_time: r.ingest_time, source: r.sensor_id, value: r.value_c })
    .collect();
    let flows = sqlx::query_as::<_, FlowRow>(
        "SELECT device_time, ingest_time, meter_id, value_lph FROM telemetry_flow WHERE device_time BETWEEN $1 AND $2 ORDER BY device_time",
    )
    .bind(start)
    .bind(end)
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|r| Sample { device_time: r.device_time, ingest_time: r.ingest_time, source: r.meter_id, value: r.value_lph })
    .collect();
    let diverts = sqlx::query_as::<_, DivertRow>(
        "SELECT device_time, ingest_time, valve_id, position FROM telemetry_divert WHERE device_time BETWEEN $1 AND $2 ORDER BY device_time",
    )
    .bind(start)
    .bind(end)
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|r| Sample {
        device_time: r.device_time,
        ingest_time: r.ingest_time,
        source: r.valve_id,
        value: DivertPosition::parse(&r.position).unwrap_or(DivertPosition::Forward),
    })
    .collect();
    Ok((temps, flows, diverts))
}

// ---------- 批次 ----------

pub async fn create_batch(pool: &PgPool, product_name: &str) -> Result<Batch, sqlx::Error> {
    sqlx::query_as::<_, Batch>(
        "INSERT INTO batches (product_name) VALUES ($1) RETURNING id, product_name, started_at, ended_at, status",
    )
    .bind(product_name)
    .fetch_one(pool)
    .await
}

pub async fn close_batch(pool: &PgPool, id: i64) -> Result<Option<Batch>, sqlx::Error> {
    sqlx::query_as::<_, Batch>(
        "UPDATE batches SET ended_at = now(), status = 'closed' WHERE id = $1 AND ended_at IS NULL RETURNING id, product_name, started_at, ended_at, status",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
}

pub async fn get_batch(pool: &PgPool, id: i64) -> Result<Option<Batch>, sqlx::Error> {
    sqlx::query_as::<_, Batch>(
        "SELECT id, product_name, started_at, ended_at, status FROM batches WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
}

pub async fn list_batches(pool: &PgPool) -> Result<Vec<Batch>, sqlx::Error> {
    sqlx::query_as::<_, Batch>(
        "SELECT id, product_name, started_at, ended_at, status FROM batches ORDER BY id DESC LIMIT 100",
    )
    .fetch_all(pool)
    .await
}

// ---------- 操作员事件 ----------

pub async fn add_event(
    pool: &PgPool,
    batch_id: i64,
    kind: EventKind,
    at: DateTime<Utc>,
    note: Option<&str>,
    marked_by: &str,
) -> Result<OperatorEvent, sqlx::Error> {
    let row = sqlx::query(
        "INSERT INTO operator_events (batch_id, kind, at, note, marked_by) VALUES ($1,$2,$3,$4,$5) RETURNING id",
    )
    .bind(batch_id)
    .bind(kind.as_str())
    .bind(at)
    .bind(note)
    .bind(marked_by)
    .fetch_one(pool)
    .await?;
    Ok(OperatorEvent {
        id: row.get("id"),
        batch_id,
        kind,
        at,
        note: note.map(|s| s.to_string()),
        marked_by: marked_by.to_string(),
    })
}

pub async fn list_events(pool: &PgPool, batch_id: i64) -> Result<Vec<OperatorEvent>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT id, batch_id, kind, at, note, marked_by FROM operator_events WHERE batch_id = $1 ORDER BY at",
    )
    .bind(batch_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| OperatorEvent {
            id: r.get("id"),
            batch_id: r.get("batch_id"),
            kind: EventKind::parse(r.get::<&str, _>("kind")).unwrap_or(EventKind::Sample),
            at: r.get("at"),
            note: r.get("note"),
            marked_by: r.get("marked_by"),
        })
        .collect())
}

// ---------- 冻结配置 ----------

pub async fn freeze_tube_config(
    pool: &PgPool,
    volume_l: f64,
    frozen_by: &str,
    note: Option<&str>,
) -> Result<HoldingTubeConfig, sqlx::Error> {
    sqlx::query_as::<_, HoldingTubeConfig>(
        "INSERT INTO holding_tube_configs (volume_l, frozen_by, note) VALUES ($1,$2,$3) RETURNING id, volume_l, valid_from, frozen_by, note",
    )
    .bind(volume_l)
    .bind(frozen_by)
    .bind(note)
    .fetch_one(pool)
    .await
}

pub async fn list_tube_configs(pool: &PgPool) -> Result<Vec<HoldingTubeConfig>, sqlx::Error> {
    sqlx::query_as::<_, HoldingTubeConfig>(
        "SELECT id, volume_l, valid_from, frozen_by, note FROM holding_tube_configs ORDER BY valid_from",
    )
    .fetch_all(pool)
    .await
}

pub async fn active_tube_config(
    pool: &PgPool,
    at: DateTime<Utc>,
) -> Result<Option<HoldingTubeConfig>, sqlx::Error> {
    sqlx::query_as::<_, HoldingTubeConfig>(
        "SELECT id, volume_l, valid_from, frozen_by, note FROM holding_tube_configs WHERE valid_from <= $1 ORDER BY valid_from DESC LIMIT 1",
    )
    .bind(at)
    .fetch_optional(pool)
    .await
}

pub async fn freeze_instrument(
    pool: &PgPool,
    instrument_id: &str,
    label: &str,
    calibrated_at: DateTime<Utc>,
    frozen_by: &str,
) -> Result<InstrumentVersion, sqlx::Error> {
    sqlx::query_as::<_, InstrumentVersion>(
        "INSERT INTO instrument_versions (instrument_id, label, calibrated_at, frozen_by) VALUES ($1,$2,$3,$4) RETURNING id, instrument_id, label, calibrated_at, valid_from, frozen_by",
    )
    .bind(instrument_id)
    .bind(label)
    .bind(calibrated_at)
    .bind(frozen_by)
    .fetch_one(pool)
    .await
}

pub async fn list_instruments(pool: &PgPool) -> Result<Vec<InstrumentVersion>, sqlx::Error> {
    sqlx::query_as::<_, InstrumentVersion>(
        "SELECT id, instrument_id, label, calibrated_at, valid_from, frozen_by FROM instrument_versions ORDER BY instrument_id, valid_from",
    )
    .fetch_all(pool)
    .await
}

pub async fn freeze_spec(
    pool: &PgPool,
    min_hold_temp_c: f64,
    max_clock_skew_s: f64,
    max_gap_s: f64,
    max_divert_feedback_s: f64,
    flow_drop_ratio: f64,
    frozen_by: &str,
    note: Option<&str>,
) -> Result<ValidationSpec, sqlx::Error> {
    sqlx::query_as::<_, ValidationSpec>(
        "INSERT INTO validation_specs (min_hold_temp_c, max_clock_skew_s, max_gap_s, max_divert_feedback_s, flow_drop_ratio, frozen_by, note) VALUES ($1,$2,$3,$4,$5,$6,$7) RETURNING id, min_hold_temp_c, max_clock_skew_s, max_gap_s, max_divert_feedback_s, flow_drop_ratio, valid_from, frozen_by, note",
    )
    .bind(min_hold_temp_c)
    .bind(max_clock_skew_s)
    .bind(max_gap_s)
    .bind(max_divert_feedback_s)
    .bind(flow_drop_ratio)
    .bind(frozen_by)
    .bind(note)
    .fetch_one(pool)
    .await
}

pub async fn active_spec(
    pool: &PgPool,
    at: DateTime<Utc>,
) -> Result<Option<ValidationSpec>, sqlx::Error> {
    sqlx::query_as::<_, ValidationSpec>(
        "SELECT id, min_hold_temp_c, max_clock_skew_s, max_gap_s, max_divert_feedback_s, flow_drop_ratio, valid_from, frozen_by, note FROM validation_specs WHERE valid_from <= $1 ORDER BY valid_from DESC LIMIT 1",
    )
    .bind(at)
    .fetch_optional(pool)
    .await
}

// ---------- 工程通知 ----------

pub async fn add_notice(
    pool: &PgPool,
    kind: NoticeKind,
    instrument_id: Option<&str>,
    reported_at: DateTime<Utc>,
    message: &str,
) -> Result<i64, sqlx::Error> {
    let row = sqlx::query(
        "INSERT INTO engineering_notices (kind, instrument_id, reported_at, message) VALUES ($1,$2,$3,$4) RETURNING id",
    )
    .bind(kind.as_str())
    .bind(instrument_id)
    .bind(reported_at)
    .bind(message)
    .fetch_one(pool)
    .await?;
    Ok(row.get("id"))
}

pub async fn list_notices(pool: &PgPool) -> Result<Vec<EngineeringNotice>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT id, kind, instrument_id, reported_at, message FROM engineering_notices ORDER BY reported_at",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| EngineeringNotice {
            id: r.get("id"),
            kind: NoticeKind::parse(r.get::<&str, _>("kind")).unwrap_or(NoticeKind::TubeModified),
            instrument_id: r.get("instrument_id"),
            reported_at: r.get("reported_at"),
            message: r.get("message"),
        })
        .collect())
}

// ---------- 发现 ----------

pub async fn replace_findings(
    pool: &PgPool,
    batch_id: i64,
    findings: &[Finding],
) -> Result<Vec<StoredFinding>, sqlx::Error> {
    sqlx::query("DELETE FROM findings WHERE batch_id = $1")
        .bind(batch_id)
        .execute(pool)
        .await?;
    for f in findings {
        let (ws, we) = f.window.unzip();
        sqlx::query(
            "INSERT INTO findings (batch_id, kind, severity, window_start, window_end, message, detail) VALUES ($1,$2,$3,$4,$5,$6,$7)",
        )
        .bind(batch_id)
        .bind(f.kind.as_str())
        .bind(f.severity.as_str())
        .bind(ws)
        .bind(we)
        .bind(&f.message)
        .bind(&f.detail)
        .execute(pool)
        .await?;
    }
    list_findings(pool, batch_id).await
}

pub async fn list_findings(pool: &PgPool, batch_id: i64) -> Result<Vec<StoredFinding>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT id, batch_id, kind, severity, window_start, window_end, message, detail, detected_at FROM findings WHERE batch_id = $1 ORDER BY id",
    )
    .bind(batch_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| {
            let ws: Option<DateTime<Utc>> = r.get("window_start");
            let we: Option<DateTime<Utc>> = r.get("window_end");
            StoredFinding {
                id: r.get("id"),
                batch_id: r.get("batch_id"),
                finding: Finding {
                    kind: FindingKind::parse(r.get::<&str, _>("kind"))
                        .unwrap_or(FindingKind::EvidenceGap),
                    severity: Severity::parse(r.get::<&str, _>("severity"))
                        .unwrap_or(Severity::Info),
                    window: ws.zip(we),
                    message: r.get("message"),
                    detail: r.get("detail"),
                },
                detected_at: r.get("detected_at"),
            }
        })
        .collect())
}

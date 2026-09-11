//! HTTP API。角色:operator 仅可标记四类事件;engineer 冻结配置。
//! 不存在任何写设定值/驱动阀门的端点。
use crate::analysis::{self, CheckInput, PassageWindow};
use crate::domain::*;
use crate::repo;
use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Json},
    routing::{get, post},
    Router,
};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use sqlx::PgPool;

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/health", get(health))
        .route("/api/batches", get(list_batches).post(create_batch))
        .route("/api/batches/{id}", get(get_batch))
        .route("/api/batches/{id}/close", post(close_batch))
        .route("/api/batches/{id}/events", post(add_event))
        .route("/api/batches/{id}/analyze", post(analyze))
        .route("/api/batches/{id}/findings", get(list_findings))
        .route("/api/batches/{id}/timeline", get(timeline))
        .route("/api/batches/{id}/evidence", get(evidence))
        .route("/api/batches/{id}/passage", get(passage))
        .route(
            "/api/configs/holding-tube",
            get(list_tube_configs).post(freeze_tube),
        )
        .route("/api/configs/holding-tube/active", get(active_tube))
        .route(
            "/api/configs/instruments",
            get(list_instruments).post(freeze_instrument),
        )
        .route("/api/configs/spec", post(freeze_spec))
        .route("/api/configs/spec/active", get(active_spec))
        .route("/api/notices", get(list_notices).post(add_notice))
        .with_state(state)
}

// ---------- 错误与角色 ----------

pub enum ApiError {
    BadRequest(String),
    NotFound(String),
    Forbidden(String),
    Conflict(String),
    Internal(String),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        let (code, msg) = match self {
            ApiError::BadRequest(m) => (StatusCode::BAD_REQUEST, m),
            ApiError::NotFound(m) => (StatusCode::NOT_FOUND, m),
            ApiError::Forbidden(m) => (StatusCode::FORBIDDEN, m),
            ApiError::Conflict(m) => (StatusCode::CONFLICT, m),
            ApiError::Internal(m) => (StatusCode::INTERNAL_SERVER_ERROR, m),
        };
        (code, Json(serde_json::json!({"error": msg}))).into_response()
    }
}

impl From<sqlx::Error> for ApiError {
    fn from(e: sqlx::Error) -> Self {
        ApiError::Internal(e.to_string())
    }
}

fn role(headers: &HeaderMap) -> &str {
    headers
        .get("x-role")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("operator")
}

fn user(headers: &HeaderMap) -> String {
    headers
        .get("x-user")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("unknown")
        .to_string()
}

fn require_engineer(headers: &HeaderMap) -> Result<(), ApiError> {
    if role(headers) == "engineer" {
        Ok(())
    } else {
        Err(ApiError::Forbidden(
            "仅验证工程师可冻结容积/仪表/规格配置".into(),
        ))
    }
}

// ---------- 处理器 ----------

async fn health() -> &'static str {
    "ok"
}

#[derive(Deserialize)]
struct CreateBatchReq {
    product_name: String,
}

async fn create_batch(
    State(s): State<AppState>,
    Json(req): Json<CreateBatchReq>,
) -> Result<Json<Batch>, ApiError> {
    validate_product_name(&req.product_name).map_err(ApiError::BadRequest)?;
    Ok(Json(
        repo::create_batch(&s.pool, req.product_name.trim()).await?,
    ))
}

async fn list_batches(State(s): State<AppState>) -> Result<Json<Vec<Batch>>, ApiError> {
    Ok(Json(repo::list_batches(&s.pool).await?))
}

async fn get_batch(
    State(s): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Batch>, ApiError> {
    repo::get_batch(&s.pool, id)
        .await?
        .map(Json)
        .ok_or_else(|| ApiError::NotFound(format!("批次 {id} 不存在")))
}

async fn close_batch(
    State(s): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Batch>, ApiError> {
    repo::close_batch(&s.pool, id)
        .await?
        .map(Json)
        .ok_or_else(|| ApiError::Conflict(format!("批次 {id} 不存在或已关闭")))
}

#[derive(Deserialize)]
struct EventReq {
    kind: EventKind,
    at: Option<DateTime<Utc>>,
    note: Option<String>,
}

async fn add_event(
    State(s): State<AppState>,
    Path(id): Path<i64>,
    headers: HeaderMap,
    Json(req): Json<EventReq>,
) -> Result<Json<OperatorEvent>, ApiError> {
    // 操作员与工程师都可标记,但仅限这四类(类型系统保证)
    let at = req.at.unwrap_or_else(Utc::now);
    let ev = repo::add_event(
        &s.pool,
        id,
        req.kind,
        at,
        req.note.as_deref(),
        &user(&headers),
    )
    .await?;
    Ok(Json(ev))
}

#[derive(Deserialize)]
struct FreezeTubeReq {
    volume_l: f64,
    note: Option<String>,
}

async fn freeze_tube(
    State(s): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<FreezeTubeReq>,
) -> Result<Json<HoldingTubeConfig>, ApiError> {
    require_engineer(&headers)?;
    validate_volume_l(req.volume_l).map_err(ApiError::BadRequest)?;
    Ok(Json(
        repo::freeze_tube_config(&s.pool, req.volume_l, &user(&headers), req.note.as_deref())
            .await?,
    ))
}

async fn list_tube_configs(
    State(s): State<AppState>,
) -> Result<Json<Vec<HoldingTubeConfig>>, ApiError> {
    Ok(Json(repo::list_tube_configs(&s.pool).await?))
}

async fn active_tube(State(s): State<AppState>) -> Result<Json<Option<HoldingTubeConfig>>, ApiError> {
    Ok(Json(repo::active_tube_config(&s.pool, Utc::now()).await?))
}

#[derive(Deserialize)]
struct FreezeInstrumentReq {
    instrument_id: String,
    label: String,
    calibrated_at: DateTime<Utc>,
}

async fn freeze_instrument(
    State(s): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<FreezeInstrumentReq>,
) -> Result<Json<InstrumentVersion>, ApiError> {
    require_engineer(&headers)?;
    validate_instrument(&req.instrument_id, &req.label).map_err(ApiError::BadRequest)?;
    Ok(Json(
        repo::freeze_instrument(
            &s.pool,
            req.instrument_id.trim(),
            req.label.trim(),
            req.calibrated_at,
            &user(&headers),
        )
        .await?,
    ))
}

async fn list_instruments(
    State(s): State<AppState>,
) -> Result<Json<Vec<InstrumentVersion>>, ApiError> {
    Ok(Json(repo::list_instruments(&s.pool).await?))
}

#[derive(Deserialize)]
struct FreezeSpecReq {
    min_hold_temp_c: f64,
    max_clock_skew_s: Option<f64>,
    max_gap_s: Option<f64>,
    max_divert_feedback_s: Option<f64>,
    flow_drop_ratio: Option<f64>,
    note: Option<String>,
}

async fn freeze_spec(
    State(s): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<FreezeSpecReq>,
) -> Result<Json<ValidationSpec>, ApiError> {
    require_engineer(&headers)?;
    let max_clock_skew_s = req.max_clock_skew_s.unwrap_or(1.0);
    let max_gap_s = req.max_gap_s.unwrap_or(10.0);
    let max_divert_feedback_s = req.max_divert_feedback_s.unwrap_or(2.0);
    let flow_drop_ratio = req.flow_drop_ratio.unwrap_or(0.6);
    validate_spec_thresholds(
        req.min_hold_temp_c,
        max_clock_skew_s,
        max_gap_s,
        max_divert_feedback_s,
        flow_drop_ratio,
    )
    .map_err(ApiError::BadRequest)?;
    Ok(Json(
        repo::freeze_spec(
            &s.pool,
            req.min_hold_temp_c,
            max_clock_skew_s,
            max_gap_s,
            max_divert_feedback_s,
            flow_drop_ratio,
            &user(&headers),
            req.note.as_deref(),
        )
        .await?,
    ))
}

async fn active_spec(State(s): State<AppState>) -> Result<Json<Option<ValidationSpec>>, ApiError> {
    Ok(Json(repo::active_spec(&s.pool, Utc::now()).await?))
}

#[derive(Deserialize)]
struct NoticeReq {
    kind: NoticeKind,
    instrument_id: Option<String>,
    reported_at: Option<DateTime<Utc>>,
    message: Option<String>,
}

async fn add_notice(
    State(s): State<AppState>,
    Json(req): Json<NoticeReq>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let id = repo::add_notice(
        &s.pool,
        req.kind,
        req.instrument_id.as_deref(),
        req.reported_at.unwrap_or_else(Utc::now),
        req.message.as_deref().unwrap_or(""),
    )
    .await?;
    Ok(Json(serde_json::json!({"id": id})))
}

async fn list_notices(State(s): State<AppState>) -> Result<Json<Vec<EngineeringNotice>>, ApiError> {
    Ok(Json(repo::list_notices(&s.pool).await?))
}

async fn analyze(
    State(s): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Vec<StoredFinding>>, ApiError> {
    let batch = repo::get_batch(&s.pool, id)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("批次 {id} 不存在")))?;
    let end = batch.ended_at.unwrap_or_else(Utc::now);
    let (temps, flows, diverts) = repo::telemetry_window(&s.pool, batch.started_at, end).await?;
    let events = repo::list_events(&s.pool, id).await?;
    let notices = repo::list_notices(&s.pool).await?;
    let tube_configs = repo::list_tube_configs(&s.pool).await?;
    let instruments = repo::list_instruments(&s.pool).await?;
    let spec = repo::active_spec(&s.pool, batch.started_at)
        .await?
        .ok_or_else(|| {
            ApiError::Conflict("未冻结验证规格(低温限值与阈值),无法分析".into())
        })?;
    let input = CheckInput {
        batch_start: batch.started_at,
        batch_end: end,
        temps: &temps,
        flows: &flows,
        diverts: &diverts,
        events: &events,
        notices: &notices,
        tube_configs: &tube_configs,
        instruments: &instruments,
        spec: &spec,
    };
    let findings = analysis::run_all(&input);
    let stored = repo::replace_findings(&s.pool, id, &findings).await?;
    Ok(Json(stored))
}

async fn list_findings(
    State(s): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Vec<StoredFinding>>, ApiError> {
    Ok(Json(repo::list_findings(&s.pool, id).await?))
}

#[derive(serde::Serialize)]
struct TimelineResponse {
    batch: Batch,
    temps: Vec<Sample<f64>>,
    flows: Vec<Sample<f64>>,
    diverts: Vec<Sample<DivertPosition>>,
    events: Vec<OperatorEvent>,
    findings: Vec<StoredFinding>,
    tube_config: Option<HoldingTubeConfig>,
    spec: Option<ValidationSpec>,
    /// 本批遥测实际涉及的传感器来源(位号 + 通道 + 安装位置),供界面标注证据边界
    sensor_sites: Vec<SensorSite>,
}

async fn timeline(
    State(s): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<TimelineResponse>, ApiError> {
    let batch = repo::get_batch(&s.pool, id)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("批次 {id} 不存在")))?;
    let end = batch.ended_at.unwrap_or_else(Utc::now);
    let (temps, flows, diverts) = repo::telemetry_window(&s.pool, batch.started_at, end).await?;
    let events = repo::list_events(&s.pool, id).await?;
    let findings = repo::list_findings(&s.pool, id).await?;
    let tube_config = repo::active_tube_config(&s.pool, batch.started_at).await?;
    let spec = repo::active_spec(&s.pool, batch.started_at).await?;
    let sensor_sites = sensor_sites_of(&temps, &flows, &diverts);
    Ok(Json(TimelineResponse {
        batch,
        temps,
        flows,
        diverts,
        events,
        findings,
        tube_config,
        spec,
        sensor_sites,
    }))
}

/// 证据清单单次返回上限(防止异常长批次拖垮响应;total 仍报告全量条数)
const EVIDENCE_MAX_ROWS: usize = 5000;

#[derive(Deserialize)]
struct EvidenceQuery {
    /// temperature | flow | divert;缺省或 all 为全部通道
    channel: Option<String>,
}

#[derive(serde::Serialize)]
struct EvidenceResponse {
    batch: Batch,
    window_start: DateTime<Utc>,
    window_end: DateTime<Utc>,
    total: usize,
    truncated: bool,
    rows: Vec<EvidenceRow>,
}

/// 审核证据清单:逐条列出批次窗口内的温度/流量/分流样本,
/// 每行显式标注所属产品批(id + 产品名)与传感器安装位置 ——
/// 热历程证据的来源边界在此可逐条核对。
async fn evidence(
    State(s): State<AppState>,
    Path(id): Path<i64>,
    Query(q): Query<EvidenceQuery>,
) -> Result<Json<EvidenceResponse>, ApiError> {
    let batch = repo::get_batch(&s.pool, id)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("批次 {id} 不存在")))?;
    let channel = match q.channel.as_deref() {
        None | Some("all") => None,
        Some(c) => Some(Channel::parse(c).ok_or_else(|| {
            ApiError::BadRequest(format!("未知通道「{c}」(应为 temperature|flow|divert)"))
        })?),
    };
    let end = batch.ended_at.unwrap_or_else(Utc::now);
    let (temps, flows, diverts) = repo::telemetry_window(&s.pool, batch.started_at, end).await?;
    let mut rows = evidence_rows(id, &batch.product_name, &temps, &flows, &diverts);
    if let Some(c) = channel {
        rows.retain(|r| r.channel == c);
    }
    let total = rows.len();
    let truncated = total > EVIDENCE_MAX_ROWS;
    rows.truncate(EVIDENCE_MAX_ROWS);
    let window_start = batch.started_at;
    Ok(Json(EvidenceResponse {
        batch,
        window_start,
        window_end: end,
        total,
        truncated,
        rows,
    }))
}

#[derive(Deserialize)]
struct PassageQuery {
    entry: DateTime<Utc>,
}

async fn passage(
    State(s): State<AppState>,
    Path(id): Path<i64>,
    Query(q): Query<PassageQuery>,
) -> Result<Json<PassageWindow>, ApiError> {
    let batch = repo::get_batch(&s.pool, id)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("批次 {id} 不存在")))?;
    let end = batch.ended_at.unwrap_or_else(Utc::now);
    let cfg = repo::active_tube_config(&s.pool, batch.started_at)
        .await?
        .ok_or_else(|| ApiError::Conflict("无已冻结的保持管容积".into()))?;
    let (_, flows, diverts) = repo::telemetry_window(&s.pool, batch.started_at, end).await?;
    let result = analysis::passage_exit_time(q.entry, &flows, cfg.volume_l);
    let (interpretable, obstruction) = match &result {
        analysis::PassageResult::Exited { exit, .. } => {
            if analysis::overlaps_divert(q.entry, *exit, &diverts) {
                (false, Some("divert_overlap".to_string()))
            } else {
                (true, None)
            }
        }
        analysis::PassageResult::HeldInTube { .. } => (false, Some("held_in_tube".to_string())),
        analysis::PassageResult::InsufficientData { .. } => {
            (false, Some("insufficient_data".to_string()))
        }
    };
    Ok(Json(PassageWindow {
        entry: q.entry,
        result,
        interpretable,
        obstruction,
    }))
}

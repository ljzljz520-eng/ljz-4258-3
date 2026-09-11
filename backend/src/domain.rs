//! 领域模型:遥测镜像、批次、冻结配置、工程通知与验证发现。
//! 安全边界:本类型体系只描述"读取/镜像/比对",不存在任何写设定值或驱动阀门的概念。
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

/// 单条遥测样本。`device_time` 为设备时钟(源时间戳),`ingest_time` 为平台接收时钟。
/// TimescaleDB 超表按 device_time 分区 —— 热历程以设备时钟为准,
/// 两钟之差(时钟差)本身也是被检查对象。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sample<T> {
    pub device_time: DateTime<Utc>,
    pub ingest_time: DateTime<Utc>,
    pub source: String,
    pub value: T,
}

impl<T> Sample<T> {
    /// 时钟差 = 接收时钟 - 设备时钟(正:设备钟慢/链路延迟;负:设备钟快)
    pub fn skew(&self) -> Duration {
        self.ingest_time - self.device_time
    }
}

/// 遥测通道(证据来源类别)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Channel {
    Temperature,
    Flow,
    Divert,
}

impl Channel {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Temperature => "temperature",
            Self::Flow => "flow",
            Self::Divert => "divert",
        }
    }
    pub fn label(&self) -> &'static str {
        match self {
            Self::Temperature => "温度",
            Self::Flow => "流量",
            Self::Divert => "分流",
        }
    }
    pub fn unit(&self) -> &'static str {
        match self {
            Self::Temperature => "°C",
            Self::Flow => "L/h",
            Self::Divert => "—",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "temperature" => Some(Self::Temperature),
            "flow" => Some(Self::Flow),
            "divert" => Some(Self::Divert),
            _ => None,
        }
    }
}

/// 传感器安装位置登记(只读镜像的元数据边界)。
/// 位号 → 安装位置;未登记的位号显式返回"未登记位置" ——
/// 审核时必须能看出"这条证据的位置边界未知",而不是留空。
pub fn sensor_position(sensor_id: &str) -> &'static str {
    match sensor_id {
        "TT-101" => "保持管出口(加热段末端)",
        "FT-201" => "保持管入口(进料流量计)",
        "XV-301" => "分流阀(转向阀)",
        _ => "未登记位置",
    }
}

/// 一个传感器的完整来源描述:位号 + 通道 + 安装位置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SensorSite {
    pub sensor_id: String,
    pub channel: Channel,
    pub position: String,
}

pub fn sensor_site(sensor_id: &str, channel: Channel) -> SensorSite {
    SensorSite {
        sensor_id: sensor_id.to_string(),
        channel,
        position: sensor_position(sensor_id).to_string(),
    }
}

/// 汇总三通道实际出现的传感器来源(按位号去重),用于界面标注证据边界。
pub fn sensor_sites_of(
    temps: &[Sample<f64>],
    flows: &[Sample<f64>],
    diverts: &[Sample<DivertPosition>],
) -> Vec<SensorSite> {
    fn add(out: &mut Vec<SensorSite>, channel: Channel, id: &str) {
        if !out.iter().any(|s| s.sensor_id == id) {
            out.push(sensor_site(id, channel));
        }
    }
    let mut out = Vec::new();
    for s in temps {
        add(&mut out, Channel::Temperature, &s.source);
    }
    for s in flows {
        add(&mut out, Channel::Flow, &s.source);
    }
    for s in diverts {
        add(&mut out, Channel::Divert, &s.source);
    }
    out
}

/// 证据值:温度/流量为数值,分流为阀位
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum EvidenceValue {
    Num(f64),
    Position(DivertPosition),
}

/// 一条可逐条核对的热历程证据:样本 + 显式归属(产品批)+ 显式来源(通道/位号/安装位置)。
/// 审核界面据此回答"这条温度/流量/分流样本属于哪个产品批、哪个安装位置"。
#[derive(Debug, Clone, Serialize)]
pub struct EvidenceRow {
    pub device_time: DateTime<Utc>,
    pub ingest_time: DateTime<Utc>,
    pub channel: Channel,
    pub sensor_id: String,
    pub position: String,
    pub value: EvidenceValue,
    pub unit: String,
    /// 时钟差 = 接收时钟 - 设备时钟(秒)
    pub skew_s: f64,
    /// 显式批次归属:该行证据所属的产品批
    pub batch_id: i64,
    pub product_name: String,
}

/// 把三通道镜像合并为逐条证据行(按设备时钟升序),每行都打上批次归属与安装位置。
/// 纯函数,无 IO。
pub fn evidence_rows(
    batch_id: i64,
    product_name: &str,
    temps: &[Sample<f64>],
    flows: &[Sample<f64>],
    diverts: &[Sample<DivertPosition>],
) -> Vec<EvidenceRow> {
    let mut rows = Vec::with_capacity(temps.len() + flows.len() + diverts.len());
    for s in temps {
        rows.push(EvidenceRow {
            device_time: s.device_time,
            ingest_time: s.ingest_time,
            channel: Channel::Temperature,
            sensor_id: s.source.clone(),
            position: sensor_position(&s.source).to_string(),
            value: EvidenceValue::Num(s.value),
            unit: Channel::Temperature.unit().to_string(),
            skew_s: s.skew().num_milliseconds() as f64 / 1000.0,
            batch_id,
            product_name: product_name.to_string(),
        });
    }
    for s in flows {
        rows.push(EvidenceRow {
            device_time: s.device_time,
            ingest_time: s.ingest_time,
            channel: Channel::Flow,
            sensor_id: s.source.clone(),
            position: sensor_position(&s.source).to_string(),
            value: EvidenceValue::Num(s.value),
            unit: Channel::Flow.unit().to_string(),
            skew_s: s.skew().num_milliseconds() as f64 / 1000.0,
            batch_id,
            product_name: product_name.to_string(),
        });
    }
    for s in diverts {
        rows.push(EvidenceRow {
            device_time: s.device_time,
            ingest_time: s.ingest_time,
            channel: Channel::Divert,
            sensor_id: s.source.clone(),
            position: sensor_position(&s.source).to_string(),
            value: EvidenceValue::Position(s.value),
            unit: Channel::Divert.unit().to_string(),
            skew_s: s.skew().num_milliseconds() as f64 / 1000.0,
            batch_id,
            product_name: product_name.to_string(),
        });
    }
    rows.sort_by_key(|r| r.device_time);
    rows
}

// ---------- 冻结输入校验(纯函数,API 层复用;错误消息直接面向操作者) ----------

/// 产品名:去空白后不能为空
pub fn validate_product_name(name: &str) -> Result<(), String> {
    if name.trim().is_empty() {
        Err("产品名不能为空".into())
    } else {
        Ok(())
    }
}

/// 保持管容积:必须为有限正数(NaN/±∞ 同样拒绝)
pub fn validate_volume_l(volume_l: f64) -> Result<(), String> {
    if !volume_l.is_finite() || volume_l <= 0.0 {
        Err("保持管容积必须为有限正数".into())
    } else {
        Ok(())
    }
}

/// 仪表版本:位号与标签去空白后均不能为空
pub fn validate_instrument(instrument_id: &str, label: &str) -> Result<(), String> {
    if instrument_id.trim().is_empty() {
        return Err("仪表位号不能为空".into());
    }
    if label.trim().is_empty() {
        return Err("仪表标签不能为空".into());
    }
    Ok(())
}

/// 验证规格:低温限值必须有限;各时间阈值必须为有限正数;流量比 ∈ (0, 1]。
/// 平台不评判限值本身的工艺合理性(那是工程师的冻结输入),但必须拒绝
/// 空/非正/非有限的阈值 —— 否则检查项会以无意义阈值静默运行。
pub fn validate_spec_thresholds(
    min_hold_temp_c: f64,
    max_clock_skew_s: f64,
    max_gap_s: f64,
    max_divert_feedback_s: f64,
    flow_drop_ratio: f64,
) -> Result<(), String> {
    if !min_hold_temp_c.is_finite() {
        return Err("低温限值必须为有限数值".into());
    }
    for (name, v) in [
        ("时钟差阈值", max_clock_skew_s),
        ("证据缺口阈值", max_gap_s),
        ("分流反馈阈值", max_divert_feedback_s),
    ] {
        if !v.is_finite() || v <= 0.0 {
            return Err(format!("{name}必须为有限正数"));
        }
    }
    if !flow_drop_ratio.is_finite() || flow_drop_ratio <= 0.0 || flow_drop_ratio > 1.0 {
        return Err("流量突降比例必须在 (0, 1] 区间".into());
    }
    Ok(())
}

/// 分流阀位置(只读镜像,平台不驱动)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DivertPosition {
    Forward,
    Divert,
}

impl DivertPosition {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Forward => "forward",
            Self::Divert => "divert",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "forward" => Some(Self::Forward),
            "divert" => Some(Self::Divert),
            _ => None,
        }
    }
}

/// 操作员只允许标记的四类事件
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    Changeover, // 换料
    Cycle,      // 循环(CIP/水循环)
    Shutdown,   // 停机
    Sample,     // 取样
}

impl EventKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Changeover => "changeover",
            Self::Cycle => "cycle",
            Self::Shutdown => "shutdown",
            Self::Sample => "sample",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "changeover" => Some(Self::Changeover),
            "cycle" => Some(Self::Cycle),
            "shutdown" => Some(Self::Shutdown),
            "sample" => Some(Self::Sample),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Batch {
    pub id: i64,
    pub product_name: String,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperatorEvent {
    pub id: i64,
    pub batch_id: i64,
    pub kind: EventKind,
    pub at: DateTime<Utc>,
    pub note: Option<String>,
    pub marked_by: String,
}

/// 保持管容积冻结配置(验证工程师)
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct HoldingTubeConfig {
    pub id: i64,
    pub volume_l: f64,
    pub valid_from: DateTime<Utc>,
    pub frozen_by: String,
    pub note: Option<String>,
}

/// 仪表版本冻结(位号标签 + 校准日期)
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct InstrumentVersion {
    pub id: i64,
    pub instrument_id: String,
    pub label: String,
    pub calibrated_at: DateTime<Utc>,
    pub valid_from: DateTime<Utc>,
    pub frozen_by: String,
}

/// 验证规格:低温限值与检查阈值。注意:全部由工程师冻结输入,
/// 平台不推导、不建议任何杀菌设定值。
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct ValidationSpec {
    pub id: i64,
    pub min_hold_temp_c: f64,
    pub max_clock_skew_s: f64,
    pub max_gap_s: f64,
    pub max_divert_feedback_s: f64,
    pub flow_drop_ratio: f64,
    pub valid_from: DateTime<Utc>,
    pub frozen_by: String,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NoticeKind {
    ProbeRecalibrated, // 探头已校准(维护事实)
    TubeModified,      // 保持管已改造(维护事实)
}

impl NoticeKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ProbeRecalibrated => "probe_recalibrated",
            Self::TubeModified => "tube_modified",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "probe_recalibrated" => Some(Self::ProbeRecalibrated),
            "tube_modified" => Some(Self::TubeModified),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineeringNotice {
    pub id: i64,
    pub kind: NoticeKind,
    pub instrument_id: Option<String>,
    pub reported_at: DateTime<Utc>,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Info,
    Warning,
    Critical,
}

impl Severity {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Warning => "warning",
            Self::Critical => "critical",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "info" => Some(Self::Info),
            "warning" => Some(Self::Warning),
            "critical" => Some(Self::Critical),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FindingKind {
    LowTemperature,       // 低温事件(前向流期间低于冻结限值)
    ClockDrift,           // 时钟差超阈
    EvidenceGap,          // 证据缺口(遥测断档)
    FlowDrop,             // 流量突降
    DivertFeedbackLate,   // 分流反馈迟到
    InstrumentLabelStale, // 校准后标签未更新
    HoldingVolumeStale,   // 保持管改造后仍用旧容积
    ProductHeld,          // 停机滞留,热历程不可解释
    PassageOverDivert,    // 通过窗与分流状态重叠
    ConfigMissing,        // 缺少冻结配置,无法解释
    // ---- 再循环产品链 ----
    RecirculationUnmetered,      // 回流量未计(回流段无流量证据,物料平衡不闭合)
    BalanceTankMixing,           // 两批在平衡罐混合(换料时回流料未走完)
    RecirculationAcrossCleaning, // 再循环跨清洗边界(清洗时回流料未走完/清洗中仍回流)
    FirstPassageTempMissing,     // 第一次通过记录缺温度(该段热暴露无法验证)
    PartialRecirculationDrawoff, // 最终产品只取部分回流料(链尾仍有未走完回流料)
}

impl FindingKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::LowTemperature => "low_temperature",
            Self::ClockDrift => "clock_drift",
            Self::EvidenceGap => "evidence_gap",
            Self::FlowDrop => "flow_drop",
            Self::DivertFeedbackLate => "divert_feedback_late",
            Self::InstrumentLabelStale => "instrument_label_stale",
            Self::HoldingVolumeStale => "holding_volume_stale",
            Self::ProductHeld => "product_held",
            Self::PassageOverDivert => "passage_over_divert",
            Self::ConfigMissing => "config_missing",
            Self::RecirculationUnmetered => "recirculation_unmetered",
            Self::BalanceTankMixing => "balance_tank_mixing",
            Self::RecirculationAcrossCleaning => "recirculation_across_cleaning",
            Self::FirstPassageTempMissing => "first_passage_temp_missing",
            Self::PartialRecirculationDrawoff => "partial_recirculation_drawoff",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "low_temperature" => Self::LowTemperature,
            "clock_drift" => Self::ClockDrift,
            "evidence_gap" => Self::EvidenceGap,
            "flow_drop" => Self::FlowDrop,
            "divert_feedback_late" => Self::DivertFeedbackLate,
            "instrument_label_stale" => Self::InstrumentLabelStale,
            "holding_volume_stale" => Self::HoldingVolumeStale,
            "product_held" => Self::ProductHeld,
            "passage_over_divert" => Self::PassageOverDivert,
            "config_missing" => Self::ConfigMissing,
            "recirculation_unmetered" => Self::RecirculationUnmetered,
            "balance_tank_mixing" => Self::BalanceTankMixing,
            "recirculation_across_cleaning" => Self::RecirculationAcrossCleaning,
            "first_passage_temp_missing" => Self::FirstPassageTempMissing,
            "partial_recirculation_drawoff" => Self::PartialRecirculationDrawoff,
            _ => return None,
        })
    }
}

/// 一条验证发现(内存形态;落库形态见 StoredFinding)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Finding {
    pub kind: FindingKind,
    pub severity: Severity,
    pub window: Option<(DateTime<Utc>, DateTime<Utc>)>,
    pub message: String,
    pub detail: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredFinding {
    pub id: i64,
    pub batch_id: i64,
    #[serde(flatten)]
    pub finding: Finding,
    pub detected_at: DateTime<Utc>,
}

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

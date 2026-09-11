//! 再循环产品链:分流阀把产品切回平衡罐时,这一段"回流料"并未离开热历程 ——
//! 它会再次进入保持管。每次回到平衡罐都形成一次**新的通过尝试**;
//! 最终去向(灌装机)的热暴露 = 链上**全部**通过尝试的并集,
//! 不得只显示最后一次合格段。
//!
//! 链重建与链级检查全部为纯函数、无 IO;输入遥测镜像与操作员事件,
//! 输出链结构(链段/通过尝试/物料平衡)与链级发现。
//!
//! 库存结算规则(保守线性近似):
//!   回流段体积在段内按时间比例计入"平衡罐未走完回流料"库存,
//!   前向段体积按时间比例从库存扣减(先走回流料,库存不为负)。
//!   该近似只用于判定"事件发生时回流料是否仍未走完",不用于计算停留时间。
use crate::analysis::checks::{evidence_json, mk};
use crate::analysis::passage::{divert_at, rate_opt_at};
use crate::domain::*;
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::json;

/// 库存结算 epsilon(L):低于该值视为回流料已走完
pub const CHAIN_EPS_L: f64 = 0.5;

/// 一段热暴露记录:温度通道在该窗口内的统计(样本数 = 0 则整段无温度证据)
#[derive(Debug, Clone, Serialize)]
pub struct HeatExposure {
    pub min_c: f64,
    pub avg_c: f64,
    pub samples: usize,
}

/// 链段类别:前向去灌装机 / 分流回平衡罐
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LegKind {
    Forward,
    Return,
}

impl LegKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Forward => "forward",
            Self::Return => "return",
        }
    }
    pub fn label(&self) -> &'static str {
        match self {
            Self::Forward => "前向去灌装机",
            Self::Return => "分流回平衡罐",
        }
    }
}

/// 链段:相邻分流切换之间的一段产品流(前向段或回流段)
#[derive(Debug, Clone, Serialize)]
pub struct ChainLeg {
    /// 所属通过尝试序号(1 起);回流段结束后序号 +1
    pub seq: u32,
    pub kind: LegKind,
    pub window: (DateTime<Utc>, DateTime<Utc>),
    /// 段内流过体积(阶梯保持积分;无流量证据时按段前最后样本外推,仅供库存结算)
    pub volume_l: f64,
    /// 段内是否存在流量样本 —— 回流段无样本即"回流量未计",必须显式,不得静默
    pub metered: bool,
    /// 该段温度暴露;None = 段内无温度记录
    pub exposure: Option<HeatExposure>,
}

/// 一次通过尝试:同一序号下的连续链段(前向段 + 紧随的回流段)。
/// 每次回到平衡罐(回流段结束)→ 序号 +1,形成新的通过尝试。
#[derive(Debug, Clone, Serialize)]
pub struct PassageAttempt {
    pub seq: u32,
    pub window: (DateTime<Utc>, DateTime<Utc>),
    /// 本次通过中前向去灌装机的体积
    pub forward_l: f64,
    /// 本次通过中被分流回平衡罐的体积
    pub returned_l: f64,
    /// 本次通过的温度暴露;None = 该次通过缺温度记录,热暴露无法验证
    pub exposure: Option<HeatExposure>,
}

/// 再循环产品链:整批的链段序列 + 通过尝试 + 物料平衡。
/// `attempts` 即最终去向保留的全部热暴露清单 —— 任何一次通过都不得被丢弃。
#[derive(Debug, Clone, Serialize)]
pub struct RecirculationChain {
    pub window: (DateTime<Utc>, DateTime<Utc>),
    pub legs: Vec<ChainLeg>,
    pub attempts: Vec<PassageAttempt>,
    /// 前向去灌装机总量
    pub forward_volume_l: f64,
    /// 分流回平衡罐总量
    pub returned_volume_l: f64,
    /// 链尾仍未走完的回流料(最终产品只取部分回流料时 > 0)
    pub pending_return_l: f64,
    /// 证据来源:本链实际涉及的流量/温度位号(供审核核对证据边界)
    pub flow_sensors: Vec<String>,
    pub temp_sensors: Vec<String>,
}

/// 由遥测镜像重建再循环产品链。输入必须按 device_time 升序。
pub fn build_chain(
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    flows: &[Sample<f64>],
    diverts: &[Sample<DivertPosition>],
    temps: &[Sample<f64>],
) -> RecirculationChain {
    let flow_sensors = dedup_sources(flows);
    let temp_sensors = dedup_sources(temps);
    if end <= start {
        return RecirculationChain {
            window: (start, end),
            legs: Vec::new(),
            attempts: Vec::new(),
            forward_volume_l: 0.0,
            returned_volume_l: 0.0,
            pending_return_l: 0.0,
            flow_sensors,
            temp_sensors,
        };
    }

    // 1) 分流状态翻转点把批次窗口切成交替的前向/回流段
    let mut bounds = vec![start];
    let mut kinds = vec![match divert_at(diverts, start) {
        DivertPosition::Forward => LegKind::Forward,
        DivertPosition::Divert => LegKind::Return,
    }];
    let mut state = kinds[0];
    for s in diverts
        .iter()
        .filter(|s| s.device_time > start && s.device_time < end)
    {
        let k = match s.value {
            DivertPosition::Forward => LegKind::Forward,
            DivertPosition::Divert => LegKind::Return,
        };
        if k != state {
            state = k;
            bounds.push(s.device_time);
            kinds.push(k);
        }
    }
    bounds.push(end);

    // 2) 逐段积分流量、统计温度暴露;每次回流结束 → 新的通过尝试
    let mut legs: Vec<ChainLeg> = Vec::new();
    let mut seq = 1u32;
    for (i, kind) in kinds.iter().enumerate() {
        let (a, b) = (bounds[i], bounds[i + 1]);
        if b <= a {
            continue;
        }
        let (volume_l, metered) = integrate_flow(flows, a, b);
        legs.push(ChainLeg {
            seq,
            kind: *kind,
            window: (a, b),
            volume_l,
            metered,
            exposure: exposure_of(temps, a, b),
        });
        if *kind == LegKind::Return {
            seq += 1;
        }
    }

    // 3) 通过尝试汇总:最终去向保留每一次通过的热暴露,不得只留最后一次合格段
    let mut attempts = Vec::new();
    for s in 1..=seq {
        let mine: Vec<&ChainLeg> = legs.iter().filter(|l| l.seq == s).collect();
        if mine.is_empty() {
            continue;
        }
        let window = (mine.first().unwrap().window.0, mine.last().unwrap().window.1);
        attempts.push(PassageAttempt {
            seq: s,
            window,
            forward_l: mine
                .iter()
                .filter(|l| l.kind == LegKind::Forward)
                .map(|l| l.volume_l)
                .sum(),
            returned_l: mine
                .iter()
                .filter(|l| l.kind == LegKind::Return)
                .map(|l| l.volume_l)
                .sum(),
            exposure: exposure_of(temps, window.0, window.1),
        });
    }

    let forward_volume_l = legs
        .iter()
        .filter(|l| l.kind == LegKind::Forward)
        .map(|l| l.volume_l)
        .sum();
    let returned_volume_l = legs
        .iter()
        .filter(|l| l.kind == LegKind::Return)
        .map(|l| l.volume_l)
        .sum();
    let pending_return_l = inventory_at(&legs, end);

    RecirculationChain {
        window: (start, end),
        legs,
        attempts,
        forward_volume_l,
        returned_volume_l,
        pending_return_l,
        flow_sensors,
        temp_sensors,
    }
}

fn dedup_sources<T>(samples: &[Sample<T>]) -> Vec<String> {
    let mut out: Vec<String> = samples.iter().map(|s| s.source.clone()).collect();
    out.sort();
    out.dedup();
    out
}

/// 阶梯保持积分 [a, b] 内的流过体积(L);metered = 段内存在流量样本。
/// 段内无样本时仍按段前最后一个样本外推(体积仅供库存结算),
/// 但 metered=false 必须显式 —— 回流量未计是链级缺陷,不得静默。
pub fn integrate_flow(
    flows: &[Sample<f64>],
    a: DateTime<Utc>,
    b: DateTime<Utc>,
) -> (f64, bool) {
    let metered = flows
        .iter()
        .any(|s| s.device_time >= a && s.device_time <= b);
    let mut vol = 0.0_f64;
    let mut t_prev = a;
    for s in flows
        .iter()
        .filter(|s| s.device_time > a && s.device_time <= b)
    {
        if let Some(r) = rate_opt_at(flows, t_prev) {
            vol += r.max(0.0) * (s.device_time - t_prev).num_milliseconds() as f64 / 3_600_000.0;
        }
        t_prev = s.device_time;
    }
    if let Some(r) = rate_opt_at(flows, t_prev) {
        vol += r.max(0.0) * (b - t_prev).num_milliseconds() as f64 / 3_600_000.0;
    }
    (vol, metered)
}

/// 窗口内温度暴露统计;无任何温度样本 → None(该段热暴露无法验证)
pub fn exposure_of(
    temps: &[Sample<f64>],
    a: DateTime<Utc>,
    b: DateTime<Utc>,
) -> Option<HeatExposure> {
    let mut n = 0usize;
    let mut min_c = f64::INFINITY;
    let mut sum = 0.0;
    for s in temps
        .iter()
        .filter(|s| s.device_time >= a && s.device_time <= b)
    {
        n += 1;
        min_c = min_c.min(s.value);
        sum += s.value;
    }
    (n > 0).then_some(HeatExposure {
        min_c,
        avg_c: sum / n as f64,
        samples: n,
    })
}

/// t 时刻平衡罐内未走完的回流料库存(L)。
/// 结算规则:回流段体积按时间比例计入库存,前向段体积按时间比例扣减
/// (先走回流料,库存不为负)。段内线性是保守近似,只用于事件时刻判定。
pub fn inventory_at(legs: &[ChainLeg], t: DateTime<Utc>) -> f64 {
    let mut inv = 0.0_f64;
    for leg in legs {
        let (a, b) = leg.window;
        if a >= t {
            break;
        }
        let span_ms = (b - a).num_milliseconds();
        if span_ms <= 0 {
            continue;
        }
        let frac = ((t - a).num_milliseconds().min(span_ms)) as f64 / span_ms as f64;
        let v = leg.volume_l * frac;
        match leg.kind {
            LegKind::Return => inv += v,
            LegKind::Forward => inv = (inv - v).max(0.0),
        }
    }
    inv
}

/// 链级检查(全部纯函数):
/// 回流量未计 / 两批在平衡罐混合 / 再循环跨清洗边界 /
/// 第一次通过记录缺温度 / 最终产品只取部分回流料。
pub fn check_chain(chain: &RecirculationChain, events: &[OperatorEvent]) -> Vec<Finding> {
    let mut out = Vec::new();
    let (start, end) = chain.window;
    let flow_sensors: Vec<&str> = chain.flow_sensors.iter().map(|s| s.as_str()).collect();
    let temp_sensors: Vec<&str> = chain.temp_sensors.iter().map(|s| s.as_str()).collect();

    // 1) 回流量未计:回流段内无流量样本 → 物料平衡不闭合
    for leg in chain
        .legs
        .iter()
        .filter(|l| l.kind == LegKind::Return && !l.metered)
    {
        out.push(mk(
            FindingKind::RecirculationUnmetered,
            Severity::Critical,
            Some(leg.window),
            format!(
                "第 {} 次通过的回流段无流量证据:回流量未计,产品链物料平衡不闭合",
                leg.seq
            ),
            json!({
                "seq": leg.seq,
                "evidence": evidence_json(Channel::Flow, flow_sensors.clone()),
            }),
        ));
    }

    // 2) 两批在平衡罐混合:换料时罐内仍有未走完的回流料
    for ev in events
        .iter()
        .filter(|e| e.kind == EventKind::Changeover && e.at >= start && e.at <= end)
    {
        let pending = inventory_at(&chain.legs, ev.at);
        if pending > CHAIN_EPS_L {
            out.push(mk(
                FindingKind::BalanceTankMixing,
                Severity::Critical,
                Some((ev.at, ev.at)),
                format!(
                    "换料时平衡罐内仍有 {pending:.1} L 回流料未走完:两批在平衡罐混合,热历程归属交叉"
                ),
                json!({
                    "changeover_at": ev.at,
                    "pending_return_l": pending,
                    "marked_by": ev.marked_by,
                    "evidence": evidence_json(Channel::Flow, flow_sensors.clone()),
                }),
            ));
        }
    }

    // 3) 再循环跨清洗边界:清洗开始时回流料未走完,或清洗期间仍在回流
    for ev in events
        .iter()
        .filter(|e| e.kind == EventKind::Cycle && e.at >= start && e.at <= end)
    {
        let pending = inventory_at(&chain.legs, ev.at);
        let returning = chain.legs.iter().any(|l| {
            l.kind == LegKind::Return && l.window.0 <= ev.at && ev.at < l.window.1
        });
        if pending > CHAIN_EPS_L || returning {
            out.push(mk(
                FindingKind::RecirculationAcrossCleaning,
                Severity::Critical,
                Some((ev.at, ev.at)),
                format!(
                    "回流料跨越清洗边界:清洗开始时平衡罐仍有 {pending:.1} L 回流产品{},热历程不得跨清洗拼接",
                    if returning { "(清洗期间仍在回流)" } else { "" }
                ),
                json!({
                    "cycle_at": ev.at,
                    "pending_return_l": pending,
                    "returning_during_cycle": returning,
                    "marked_by": ev.marked_by,
                    "evidence": evidence_json(Channel::Flow, flow_sensors.clone()),
                }),
            ));
        }
    }

    // 4) 第一次通过记录缺温度:该段热暴露无法验证;
    //    最终去向保留全部通过的热暴露,不得只显示最后一次合格段
    if let Some(first) = chain.attempts.first() {
        if first.exposure.is_none() {
            out.push(mk(
                FindingKind::FirstPassageTempMissing,
                Severity::Critical,
                Some(first.window),
                "第一次通过无温度记录:该段热暴露无法验证;最终去向保留全部通过的热暴露,不得只显示最后一次合格段".to_string(),
                json!({
                    "seq": first.seq,
                    "evidence": evidence_json(Channel::Temperature, temp_sensors.clone()),
                }),
            ));
        }
    }

    // 5) 最终产品只取部分回流料:链尾仍有未走完的回流料
    if chain.pending_return_l > CHAIN_EPS_L {
        let last_return_end = chain
            .legs
            .iter()
            .filter(|l| l.kind == LegKind::Return)
            .last()
            .map(|l| l.window.1)
            .unwrap_or(start);
        out.push(mk(
            FindingKind::PartialRecirculationDrawoff,
            Severity::Warning,
            Some((last_return_end, end)),
            format!(
                "最终产品只取部分回流料:{:.1} L 回流料未进入最终去向;其热暴露不计入最终产品,但须保留在去向台账",
                chain.pending_return_l
            ),
            json!({
                "pending_return_l": chain.pending_return_l,
                "returned_volume_l": chain.returned_volume_l,
                "forward_volume_l": chain.forward_volume_l,
                "evidence": evidence_json(Channel::Flow, flow_sensors.clone()),
            }),
        ));
    }

    out
}

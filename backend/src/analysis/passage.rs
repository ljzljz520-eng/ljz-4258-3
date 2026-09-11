//! 保持段通过窗重建:以"冻结容积 ÷ 实际流量"对时间积分,
//! 计算产品从进入保持管到离开保持管的出口时刻。
//! 纯函数、无 IO;流量按阶梯(采样保持)插值。
use crate::domain::{DivertPosition, Sample};
use chrono::{DateTime, Duration, Utc};
use serde::Serialize;

/// 低于该流量视为停流(L/h)
pub const FLOW_EPS_LPH: f64 = 1.0;

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum PassageResult {
    /// 成功积满容积:给出出口时刻与实际停留秒数
    Exited { exit: DateTime<Utc>, residence_s: f64 },
    /// 流量归零(停机),产品滞留管内,热历程不可解释
    HeldInTube { accumulated_l: f64 },
    /// 数据不足以积满容积(批次末端/证据缺口)
    InsufficientData { accumulated_l: f64 },
}

/// 阶梯保持:t 时刻的流量 = device_time <= t 的最后一个样本值。
/// 返回 None 表示"尚无数据" —— 与"实测零流量"严格区分:
/// 前者是证据问题,后者才是停流。
pub fn rate_opt_at(flow: &[Sample<f64>], t: DateTime<Utc>) -> Option<f64> {
    flow.iter()
        .rev()
        .find(|s| s.device_time <= t)
        .map(|s| s.value)
}

/// 兼容包装:无数据时按 0 处理(仅用于展示,不用于判定)
pub fn rate_at(flow: &[Sample<f64>], t: DateTime<Utc>) -> f64 {
    rate_opt_at(flow, t).unwrap_or(0.0)
}

/// 容积积分法:从 entry 起累计流过体积,达到 volume_l 即出口。
/// 输入 flow 必须按 device_time 升序。
/// 验证语义:积分路径上一旦遇到停流区间(流量 <= FLOW_EPS_LPH),
/// 立即判为滞留 —— 即使之后流量恢复,该段热历程也不再是
/// "连续流通过保持管"的已验证状态,不可解释。
pub fn passage_exit_time(
    entry: DateTime<Utc>,
    flow: &[Sample<f64>],
    volume_l: f64,
) -> PassageResult {
    if volume_l <= 0.0 {
        return PassageResult::InsufficientData { accumulated_l: 0.0 };
    }
    let mut accumulated = 0.0_f64;
    let mut t_prev = entry;
    for s in flow.iter().filter(|s| s.device_time > entry) {
        match rate_opt_at(flow, t_prev) {
            None => {
                // 进入时刻早于首个样本:证据未覆盖,不累计也不判滞留
                t_prev = s.device_time;
                continue;
            }
            Some(r) if r <= FLOW_EPS_LPH => {
                // 实测停流区间:产品滞留管内
                return PassageResult::HeldInTube {
                    accumulated_l: accumulated,
                };
            }
            Some(r) => {
                let dt_h = (s.device_time - t_prev).num_milliseconds() as f64 / 3_600_000.0;
                accumulated += r.max(0.0) * dt_h;
                t_prev = s.device_time;
            }
        }
        if accumulated >= volume_l {
            // 在本段内线性回插出口时刻
            let rate_now = rate_opt_at(flow, t_prev).unwrap_or(0.0).max(f64::EPSILON);
            let overshoot_l = accumulated - volume_l;
            let back_ms = (overshoot_l / rate_now * 3_600_000.0) as i64;
            let exit = s.device_time - Duration::milliseconds(back_ms);
            return PassageResult::Exited {
                exit,
                residence_s: (exit - entry).num_milliseconds() as f64 / 1000.0,
            };
        }
    }
    match rate_opt_at(flow, t_prev) {
        Some(r) if r <= FLOW_EPS_LPH => PassageResult::HeldInTube {
            accumulated_l: accumulated,
        },
        _ => PassageResult::InsufficientData {
            accumulated_l: accumulated,
        },
    }
}

/// 一个通过窗:进入时刻 + 重建结果 + 可解释性
#[derive(Debug, Clone, Serialize)]
pub struct PassageWindow {
    pub entry: DateTime<Utc>,
    pub result: PassageResult,
    pub interpretable: bool,
    pub obstruction: Option<String>,
}

/// 在 [start, end] 内按 step 采样进入时刻,重建整批通过窗序列。
/// 与分流状态重叠的窗口标记为不可解释(流向变化破坏热历程归因)。
pub fn reconstruct_passage_windows(
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    step: Duration,
    flow: &[Sample<f64>],
    divert: &[Sample<DivertPosition>],
    volume_l: f64,
) -> Vec<PassageWindow> {
    let mut out = Vec::new();
    let mut t = start;
    while t <= end {
        let result = passage_exit_time(t, flow, volume_l);
        let (interpretable, obstruction) = match &result {
            PassageResult::Exited { exit, .. } => {
                if overlaps_divert(t, *exit, divert) {
                    (false, Some("divert_overlap".to_string()))
                } else {
                    (true, None)
                }
            }
            PassageResult::HeldInTube { .. } => (false, Some("held_in_tube".to_string())),
            PassageResult::InsufficientData { .. } => {
                (false, Some("insufficient_data".to_string()))
            }
        };
        out.push(PassageWindow {
            entry: t,
            result,
            interpretable,
            obstruction,
        });
        t += step;
    }
    out
}

pub fn divert_at(divert: &[Sample<DivertPosition>], t: DateTime<Utc>) -> DivertPosition {
    divert
        .iter()
        .rev()
        .find(|s| s.device_time <= t)
        .map(|s| s.value)
        .unwrap_or(DivertPosition::Forward)
}

/// [from, to] 内是否出现过分流(含起点即处于分流)
pub fn overlaps_divert(
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    divert: &[Sample<DivertPosition>],
) -> bool {
    if divert_at(divert, from) == DivertPosition::Divert {
        return true;
    }
    divert
        .iter()
        .any(|s| s.device_time > from && s.device_time <= to && s.value == DivertPosition::Divert)
}

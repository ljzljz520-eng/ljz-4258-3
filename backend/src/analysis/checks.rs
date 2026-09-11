//! 平台检查项:低温事件、时钟差、证据缺口、流量突降、分流反馈迟到、
//! 校准标签滞后、保持管容积滞后、停机滞留。全部纯函数。
use crate::analysis::passage::{divert_at, passage_exit_time, PassageResult, FLOW_EPS_LPH};
use crate::domain::*;
use chrono::{DateTime, Duration, Utc};
use serde_json::json;

/// 一次分析所需的全部输入(遥测按 device_time 升序)
pub struct CheckInput<'a> {
    pub batch_start: DateTime<Utc>,
    pub batch_end: DateTime<Utc>,
    pub temps: &'a [Sample<f64>],
    pub flows: &'a [Sample<f64>],
    pub diverts: &'a [Sample<DivertPosition>],
    pub events: &'a [OperatorEvent],
    pub notices: &'a [EngineeringNotice],
    pub tube_configs: &'a [HoldingTubeConfig],
    pub instruments: &'a [InstrumentVersion],
    pub spec: &'a ValidationSpec,
}

pub fn run_all(input: &CheckInput) -> Vec<Finding> {
    let mut out = Vec::new();
    let cfg = input
        .tube_configs
        .iter()
        .filter(|c| c.valid_from <= input.batch_start)
        .max_by_key(|c| c.valid_from);
    match cfg {
        None => out.push(Finding {
            kind: FindingKind::ConfigMissing,
            severity: Severity::Critical,
            window: Some((input.batch_start, input.batch_end)),
            message: "批次开始时无已冻结的保持管容积,通过窗无法重建".into(),
            detail: json!({}),
        }),
        Some(c) => {
            out.extend(check_product_held(
                input.batch_start,
                input.batch_end,
                input.events,
                input.flows,
                c.volume_l,
            ));
        }
    }
    out.extend(check_low_temperature(
        input.temps,
        input.diverts,
        input.spec.min_hold_temp_c,
    ));
    out.extend(check_clock_drift(input.temps, input.flows, input.diverts, input.spec));
    out.extend(check_evidence_gaps(
        input.batch_start,
        input.batch_end,
        input.temps,
        input.flows,
        input.spec.max_gap_s,
    ));
    out.extend(check_flow_drop(input.flows, input.spec.flow_drop_ratio));
    out.extend(check_divert_feedback_delay(
        input.diverts,
        input.spec.max_divert_feedback_s,
    ));
    out.extend(check_instrument_label(input.notices, input.instruments));
    out.extend(check_holding_volume(input.notices, input.tube_configs));
    out
}

fn mk(
    kind: FindingKind,
    severity: Severity,
    window: Option<(DateTime<Utc>, DateTime<Utc>)>,
    message: impl Into<String>,
    detail: serde_json::Value,
) -> Finding {
    Finding {
        kind,
        severity,
        window,
        message: message.into(),
        detail,
    }
}

/// 证据来源描述:让每条发现的"结论依据哪些传感器、哪个安装位置"显式可核对。
/// 位号去重排序;安装位置来自 sensor_position 登记(未登记位号显式标注)。
fn evidence_json(channel: Channel, mut sensors: Vec<&str>) -> serde_json::Value {
    sensors.sort();
    sensors.dedup();
    let positions: Vec<&str> = sensors.iter().map(|s| sensor_position(s)).collect();
    json!({
        "channel": channel.as_str(),
        "channel_label": channel.label(),
        "sensors": sensors,
        "positions": positions,
    })
}

/// 低温事件:仅在前向流(Forward)期间判定;分流期间低温属预期(产品被切走)。
pub fn check_low_temperature(
    temps: &[Sample<f64>],
    divert: &[Sample<DivertPosition>],
    min_temp_c: f64,
) -> Vec<Finding> {
    let mut out = Vec::new();
    let mut group: Vec<&Sample<f64>> = Vec::new();
    let mut flush = |group: &mut Vec<&Sample<f64>>| {
        if group.is_empty() {
            return;
        }
        let min_v = group.iter().map(|s| s.value).fold(f64::INFINITY, f64::min);
        let sensors: Vec<&str> = group.iter().map(|s| s.source.as_str()).collect();
        out.push(mk(
            FindingKind::LowTemperature,
            Severity::Critical,
            Some((group.first().unwrap().device_time, group.last().unwrap().device_time)),
            format!(
                "前向流期间保持段温度低于冻结限值 {min_temp_c:.1}°C(最低 {min_v:.1}°C,{} 个样本)",
                group.len()
            ),
            json!({
                "min_c": min_v,
                "limit_c": min_temp_c,
                "samples": group.len(),
                "evidence": evidence_json(Channel::Temperature, sensors),
            }),
        ));
        group.clear();
    };
    for s in temps {
        let forward = divert_at(divert, s.device_time) == DivertPosition::Forward;
        if forward && s.value < min_temp_c {
            group.push(s);
        } else {
            flush(&mut group);
        }
    }
    flush(&mut group);
    out
}

/// 时钟差:任一通道 |接收钟 - 设备钟| 超阈即报(按通道聚合,取最差样本)。
pub fn check_clock_drift(
    temps: &[Sample<f64>],
    flows: &[Sample<f64>],
    diverts: &[Sample<DivertPosition>],
    spec: &ValidationSpec,
) -> Vec<Finding> {
    let mut out = Vec::new();
    let limit_ms = (spec.max_clock_skew_s * 1000.0) as i64;
    let mut scan: Vec<(String, Channel, DateTime<Utc>, i64)> = Vec::new();
    let mut collect = |skew_ms: i64, at: DateTime<Utc>, src: &str, ch: Channel| {
        if skew_ms.abs() > limit_ms {
            scan.push((src.to_string(), ch, at, skew_ms));
        }
    };
    for s in temps {
        collect(s.skew().num_milliseconds(), s.device_time, &s.source, Channel::Temperature);
    }
    for s in flows {
        collect(s.skew().num_milliseconds(), s.device_time, &s.source, Channel::Flow);
    }
    for s in diverts {
        collect(s.skew().num_milliseconds(), s.device_time, &s.source, Channel::Divert);
    }
    // 按通道聚合
    let mut sources: Vec<&str> = scan.iter().map(|(s, _, _, _)| s.as_str()).collect();
    sources.sort();
    sources.dedup();
    for src in sources {
        let mut rows: Vec<_> = scan.iter().filter(|(s, _, _, _)| s.as_str() == src).collect();
        rows.sort_by_key(|(_, _, _, k)| -k.abs());
        let worst = rows[0];
        let ch = worst.1;
        let position = sensor_position(src);
        out.push(mk(
            FindingKind::ClockDrift,
            Severity::Warning,
            Some((worst.2, worst.2)),
            format!(
                "通道 {src}({position})时钟差超阈:最差 {:.1}s(阈值 {:.1}s),共 {} 个样本",
                worst.3 as f64 / 1000.0,
                spec.max_clock_skew_s,
                rows.len()
            ),
            json!({
                "source": src,
                "channel": ch.as_str(),
                "position": position,
                "worst_skew_s": worst.3 as f64 / 1000.0,
                "samples": rows.len(),
                "evidence": evidence_json(ch, vec![src]),
            }),
        ));
    }
    out
}

/// 证据缺口:批次窗口内相邻样本 device_time 断档超阈(温度/流量两通道)。
pub fn check_evidence_gaps(
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    temps: &[Sample<f64>],
    flows: &[Sample<f64>],
    max_gap_s: f64,
) -> Vec<Finding> {
    let mut out = Vec::new();
    let limit = Duration::milliseconds((max_gap_s * 1000.0) as i64);
    let gaps_of = |series: &[Sample<f64>], ch: Channel, out: &mut Vec<Finding>| {
        let name = ch.label();
        let sensors: Vec<&str> = series.iter().map(|s| s.source.as_str()).collect();
        let mut prev: Option<DateTime<Utc>> = Some(start);
        for s in series {
            if let Some(p) = prev {
                if s.device_time - p > limit {
                    out.push(mk(
                        FindingKind::EvidenceGap,
                        Severity::Warning,
                        Some((p, s.device_time)),
                        format!(
                            "{name} 通道证据缺口 {:.0}s(阈值 {max_gap_s:.0}s)",
                            (s.device_time - p).num_milliseconds() as f64 / 1000.0
                        ),
                        json!({
                            "channel": ch.as_str(),
                            "gap_s": (s.device_time - p).num_milliseconds() as f64 / 1000.0,
                            "evidence": evidence_json(ch, sensors.clone()),
                        }),
                    ));
                }
            }
            prev = Some(s.device_time);
        }
        if let Some(p) = prev {
            if end - p > limit {
                out.push(mk(
                    FindingKind::EvidenceGap,
                    Severity::Warning,
                    Some((p, end)),
                    format!("{name} 通道在批次结束前断档 {:.0}s", (end - p).num_milliseconds() as f64 / 1000.0),
                    json!({
                        "channel": ch.as_str(),
                        "gap_s": (end - p).num_milliseconds() as f64 / 1000.0,
                        "evidence": evidence_json(ch, sensors.clone()),
                    }),
                ));
            }
        }
    };
    gaps_of(temps, Channel::Temperature, &mut out);
    gaps_of(flows, Channel::Flow, &mut out);
    out
}

/// 流量突降:低于批次中位流量 × ratio 的连续段。
pub fn check_flow_drop(flows: &[Sample<f64>], ratio: f64) -> Vec<Finding> {
    let mut out = Vec::new();
    let mut vals: Vec<f64> = flows
        .iter()
        .map(|s| s.value)
        .filter(|v| *v > FLOW_EPS_LPH)
        .collect();
    let Some(baseline) = median(&mut vals) else { return out };
    let threshold = baseline * ratio;
    let mut group: Vec<&Sample<f64>> = Vec::new();
    let mut flush = |group: &mut Vec<&Sample<f64>>| {
        if group.is_empty() {
            return;
        }
        let min_v = group.iter().map(|s| s.value).fold(f64::INFINITY, f64::min);
        let sensors: Vec<&str> = group.iter().map(|s| s.source.as_str()).collect();
        out.push(mk(
            FindingKind::FlowDrop,
            Severity::Warning,
            Some((group.first().unwrap().device_time, group.last().unwrap().device_time)),
            format!(
                "流量突降:最低 {min_v:.0} L/h,低于中位基线 {baseline:.0} L/h 的 {:.0}%",
                ratio * 100.0
            ),
            json!({
                "min_lph": min_v,
                "baseline_lph": baseline,
                "threshold_lph": threshold,
                "evidence": evidence_json(Channel::Flow, sensors),
            }),
        ));
        group.clear();
    };
    for s in flows {
        if s.value < threshold {
            group.push(s);
        } else {
            flush(&mut group);
        }
    }
    flush(&mut group);
    out
}

/// 分流反馈迟到:分流状态样本的(接收钟 - 设备钟)超阈。
pub fn check_divert_feedback_delay(
    diverts: &[Sample<DivertPosition>],
    max_delay_s: f64,
) -> Vec<Finding> {
    let limit_ms = (max_delay_s * 1000.0) as i64;
    let late: Vec<_> = diverts
        .iter()
        .filter(|s| s.skew().num_milliseconds() > limit_ms)
        .collect();
    if late.is_empty() {
        return Vec::new();
    }
    let worst = late
        .iter()
        .max_by_key(|s| s.skew().num_milliseconds())
        .unwrap();
    vec![mk(
        FindingKind::DivertFeedbackLate,
        Severity::Critical,
        Some((worst.device_time, worst.ingest_time)),
        format!(
            "分流反馈迟到:最差 {:.1}s(阈值 {max_delay_s:.1}s),共 {} 个迟到样本;期间低温联动无法按时归因",
            worst.skew().num_milliseconds() as f64 / 1000.0,
            late.len()
        ),
        json!({
            "worst_s": worst.skew().num_milliseconds() as f64 / 1000.0,
            "late_samples": late.len(),
            "evidence": evidence_json(Channel::Divert, late.iter().map(|s| s.source.as_str()).collect()),
        }),
    )]
}

/// 校准后标签未更新:出现"探头已校准"通知后,
/// 既无 calibrated_at 覆盖该通知的冻结版本,或新版本的标签与旧版本相同。
pub fn check_instrument_label(
    notices: &[EngineeringNotice],
    versions: &[InstrumentVersion],
) -> Vec<Finding> {
    let mut out = Vec::new();
    for n in notices
        .iter()
        .filter(|n| n.kind == NoticeKind::ProbeRecalibrated)
    {
        let Some(inst) = n.instrument_id.clone() else { continue };
        let mut vs: Vec<_> = versions
            .iter()
            .filter(|v| v.instrument_id == inst)
            .collect();
        vs.sort_by_key(|v| v.valid_from);
        match vs.iter().find(|v| v.calibrated_at >= n.reported_at) {
            None => out.push(mk(
                FindingKind::InstrumentLabelStale,
                Severity::Critical,
                Some((n.reported_at, n.reported_at)),
                format!("仪表 {inst} 校准后未冻结新版本,平台仍按旧标签/旧校准日期解释温度"),
                json!({
                    "instrument_id": inst,
                    "position": sensor_position(&inst),
                    "notice_at": n.reported_at,
                }),
            )),
            Some(v) => {
                let prev = vs
                    .iter()
                    .filter(|o| o.id != v.id && o.valid_from <= v.valid_from)
                    .last();
                if let Some(p) = prev {
                    if p.label == v.label {
                        out.push(mk(
                            FindingKind::InstrumentLabelStale,
                            Severity::Critical,
                            Some((n.reported_at, v.valid_from)),
                            format!(
                                "仪表 {inst} 校准后标签未更新:新旧版本标签均为「{}」",
                                v.label
                            ),
                            json!({
                                "instrument_id": inst,
                                "position": sensor_position(&inst),
                                "label": v.label,
                                "notice_at": n.reported_at,
                            }),
                        ));
                    }
                }
            }
        }
    }
    out
}

/// 保持管改造后仍用旧容积:出现"保持管已改造"通知后,无 valid_from 晚于通知的冻结容积。
pub fn check_holding_volume(
    notices: &[EngineeringNotice],
    configs: &[HoldingTubeConfig],
) -> Vec<Finding> {
    let mut out = Vec::new();
    for n in notices
        .iter()
        .filter(|n| n.kind == NoticeKind::TubeModified)
    {
        let refreshed = configs.iter().any(|c| c.valid_from >= n.reported_at);
        if !refreshed {
            let active = configs
                .iter()
                .filter(|c| c.valid_from <= n.reported_at)
                .max_by_key(|c| c.valid_from);
            out.push(mk(
                FindingKind::HoldingVolumeStale,
                Severity::Critical,
                Some((n.reported_at, n.reported_at)),
                format!(
                    "保持管改造后仍按旧容积 {} L 重建通过窗,停留时间结论无效",
                    active.map(|c| c.volume_l).unwrap_or(f64::NAN)
                ),
                json!({
                    "notice_at": n.reported_at,
                    "stale_volume_l": active.map(|c| c.volume_l),
                    "stale_config_id": active.map(|c| c.id),
                }),
            ));
        }
    }
    out
}

/// 停机滞留:批次活跃期内标记停机,且停机前进入的产品无法积满容积(流量归零)。
pub fn check_product_held(
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    events: &[OperatorEvent],
    flows: &[Sample<f64>],
    volume_l: f64,
) -> Vec<Finding> {
    let mut out = Vec::new();
    for e in events
        .iter()
        .filter(|e| e.kind == EventKind::Shutdown && e.at >= start && e.at <= end)
    {
        let mut vals: Vec<f64> = flows
            .iter()
            .filter(|s| s.device_time <= e.at && s.value > FLOW_EPS_LPH)
            .map(|s| s.value)
            .collect();
        let baseline = median(&mut vals).unwrap_or(0.0);
        let typical_res_s = if baseline > FLOW_EPS_LPH {
            volume_l / baseline * 3600.0
        } else {
            60.0
        };
        // 探针:停机前半个典型停留时间进入的产品 —— 停机时刻它应在管内中段
        let entry = e.at - Duration::milliseconds((typical_res_s * 500.0) as i64);
        if let PassageResult::Exited { .. } = passage_exit_time(entry, flows, volume_l) {
            continue;
        }
        let recovery = flows
            .iter()
            .find(|s| s.device_time > e.at && s.value > FLOW_EPS_LPH)
            .map(|s| s.device_time)
            .unwrap_or(end);
        out.push(mk(
            FindingKind::ProductHeld,
            Severity::Critical,
            Some((e.at, recovery)),
            "停机期间产品滞留保持管,停留时间无界,该段热历程不可解释".to_string(),
            json!({
                "shutdown_at": e.at,
                "flow_recovered_at": recovery,
                "typical_residence_s": typical_res_s,
                "evidence": evidence_json(Channel::Flow, flows.iter().map(|s| s.source.as_str()).collect()),
            }),
        ));
    }
    out
}

pub fn median(values: &mut Vec<f64>) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    Some(values[values.len() / 2])
}

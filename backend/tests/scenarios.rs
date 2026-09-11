//! 五个验证测试场景 + 基础检查项。
//! 全部为纯函数测试:不依赖数据库,直接构造遥测镜像与冻结配置。
use chrono::{DateTime, Duration, TimeZone, Utc};
use pv_backend::analysis::*;
use pv_backend::domain::*;

fn base() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 9, 1, 10, 0, 0).unwrap()
}

fn temp(at: DateTime<Utc>, v: f64) -> Sample<f64> {
    Sample {
        device_time: at,
        ingest_time: at,
        source: "TT-101".into(),
        value: v,
    }
}

fn flow(at: DateTime<Utc>, v: f64) -> Sample<f64> {
    Sample {
        device_time: at,
        ingest_time: at,
        source: "FT-201".into(),
        value: v,
    }
}

fn divert(at: DateTime<Utc>, pos: DivertPosition) -> Sample<DivertPosition> {
    Sample {
        device_time: at,
        ingest_time: at,
        source: "XV-301".into(),
        value: pos,
    }
}

fn spec() -> ValidationSpec {
    ValidationSpec {
        id: 1,
        min_hold_temp_c: 72.0,
        max_clock_skew_s: 1.0,
        max_gap_s: 10.0,
        max_divert_feedback_s: 2.0,
        flow_drop_ratio: 0.6,
        valid_from: base() - Duration::days(30),
        frozen_by: "engineer".into(),
        note: None,
    }
}

fn kinds(findings: &[Finding]) -> Vec<FindingKind> {
    findings.iter().map(|f| f.kind).collect()
}

// ---------- 场景 1:温度探头校准后标签未更新 ----------

#[test]
fn scenario_1_label_stale_after_calibration() {
    let t0 = base();
    let versions = vec![InstrumentVersion {
        id: 1,
        instrument_id: "TT-101".into(),
        label: "TT-101/A".into(),
        calibrated_at: t0 - Duration::days(365),
        valid_from: t0 - Duration::days(360),
        frozen_by: "engineer".into(),
    }];
    let notices = vec![EngineeringNotice {
        id: 1,
        kind: NoticeKind::ProbeRecalibrated,
        instrument_id: Some("TT-101".into()),
        reported_at: t0,
        message: "探头已送校并回装".into(),
    }];
    let findings = check_instrument_label(&notices, &versions);
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].kind, FindingKind::InstrumentLabelStale);
    assert_eq!(findings[0].severity, Severity::Critical);
}

#[test]
fn scenario_1b_label_unchanged_in_new_version_is_still_stale() {
    let t0 = base();
    // 校准后冻结了新版本,但标签沿用了旧标签 → 仍视为未更新
    let versions = vec![
        InstrumentVersion {
            id: 1,
            instrument_id: "TT-101".into(),
            label: "TT-101/A".into(),
            calibrated_at: t0 - Duration::days(365),
            valid_from: t0 - Duration::days(360),
            frozen_by: "engineer".into(),
        },
        InstrumentVersion {
            id: 2,
            instrument_id: "TT-101".into(),
            label: "TT-101/A".into(), // 标签未变
            calibrated_at: t0 + Duration::hours(1),
            valid_from: t0 + Duration::hours(2),
            frozen_by: "engineer".into(),
        },
    ];
    let notices = vec![EngineeringNotice {
        id: 1,
        kind: NoticeKind::ProbeRecalibrated,
        instrument_id: Some("TT-101".into()),
        reported_at: t0,
        message: String::new(),
    }];
    let findings = check_instrument_label(&notices, &versions);
    assert_eq!(findings.len(), 1, "标签沿用旧值必须仍然报警");
}

#[test]
fn scenario_1c_new_label_clears_finding() {
    let t0 = base();
    let versions = vec![
        InstrumentVersion {
            id: 1,
            instrument_id: "TT-101".into(),
            label: "TT-101/A".into(),
            calibrated_at: t0 - Duration::days(365),
            valid_from: t0 - Duration::days(360),
            frozen_by: "engineer".into(),
        },
        InstrumentVersion {
            id: 2,
            instrument_id: "TT-101".into(),
            label: "TT-101/B".into(), // 新标签
            calibrated_at: t0 + Duration::hours(1),
            valid_from: t0 + Duration::hours(2),
            frozen_by: "engineer".into(),
        },
    ];
    let notices = vec![EngineeringNotice {
        id: 1,
        kind: NoticeKind::ProbeRecalibrated,
        instrument_id: Some("TT-101".into()),
        reported_at: t0,
        message: String::new(),
    }];
    assert!(check_instrument_label(&notices, &versions).is_empty());
}

// ---------- 场景 2:流量突降 ----------

#[test]
fn scenario_2_flow_sudden_drop() {
    let t0 = base();
    let mut flows = Vec::new();
    for i in 0..=600 {
        let v = if (300..420).contains(&i) { 3_000.0 } else { 10_000.0 };
        flows.push(flow(t0 + Duration::seconds(i), v));
    }
    let findings = check_flow_drop(&flows, 0.6);
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].kind, FindingKind::FlowDrop);
    let (ws, we) = findings[0].window.unwrap();
    assert_eq!(ws, t0 + Duration::seconds(300));
    assert_eq!(we, t0 + Duration::seconds(419));

    // 通过窗:跌入段的停留时间显著大于名义值(120L / 10000L/h = 43.2s)
    let entry = t0 + Duration::seconds(280);
    match passage_exit_time(entry, &flows, 120.0) {
        PassageResult::Exited { residence_s, .. } => {
            assert!(residence_s > 60.0, "流量突降必须拉长停留时间,得到 {residence_s}");
        }
        other => panic!("应能积满容积,得到 {other:?}"),
    }
}

// ---------- 场景 3:分流反馈迟到 ----------

#[test]
fn scenario_3_divert_feedback_late() {
    let t0 = base();
    let mut late = divert(t0, DivertPosition::Divert);
    late.ingest_time = t0 + Duration::seconds(5); // 反馈迟到 5s > 阈值 2s
    let findings = check_divert_feedback_delay(&[late], 2.0);
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].kind, FindingKind::DivertFeedbackLate);
    assert_eq!(findings[0].severity, Severity::Critical);

    let on_time = divert(t0, DivertPosition::Forward);
    assert!(check_divert_feedback_delay(&[on_time], 2.0).is_empty());
}

// ---------- 场景 4:保持管改造仍用旧容积 ----------

#[test]
fn scenario_4_tube_modified_old_volume() {
    let t0 = base();
    let configs = vec![HoldingTubeConfig {
        id: 1,
        volume_l: 120.0,
        valid_from: t0 - Duration::days(30),
        frozen_by: "engineer".into(),
        note: None,
    }];
    let notices = vec![EngineeringNotice {
        id: 1,
        kind: NoticeKind::TubeModified,
        instrument_id: None,
        reported_at: t0,
        message: "保持管加长改造完成".into(),
    }];
    let findings = check_holding_volume(&notices, &configs);
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].kind, FindingKind::HoldingVolumeStale);
    assert_eq!(findings[0].severity, Severity::Critical);

    // 冻结新容积后告警消除
    let mut configs2 = configs.clone();
    configs2.push(HoldingTubeConfig {
        id: 2,
        volume_l: 165.0,
        valid_from: t0 + Duration::hours(1),
        frozen_by: "engineer".into(),
        note: Some("改造后实测容积".into()),
    });
    assert!(check_holding_volume(&notices, &configs2).is_empty());
}

// ---------- 场景 5:停机时产品滞留 ----------

#[test]
fn scenario_5_product_held_during_shutdown() {
    let t0 = base();
    let shutdown_at = t0 + Duration::seconds(300);
    let mut flows = Vec::new();
    for i in 0..=600 {
        let at = t0 + Duration::seconds(i);
        let v = if at >= shutdown_at && i <= 480 { 0.0 } else { 10_000.0 };
        flows.push(flow(at, v));
    }
    let events = vec![OperatorEvent {
        id: 1,
        batch_id: 1,
        kind: EventKind::Shutdown,
        at: shutdown_at,
        note: Some("急停".into()),
        marked_by: "operator".into(),
    }];
    let findings = check_product_held(
        t0,
        t0 + Duration::seconds(600),
        &events,
        &flows,
        120.0,
    );
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].kind, FindingKind::ProductHeld);
    assert_eq!(findings[0].severity, Severity::Critical);

    // 停机前一刻进入的产品:无法积满容积 → 滞留
    let entry = shutdown_at - Duration::seconds(30);
    assert!(matches!(
        passage_exit_time(entry, &flows, 120.0),
        PassageResult::HeldInTube { .. }
    ));
}

// ---------- 基础检查项 ----------

#[test]
fn low_temp_only_counts_during_forward_flow() {
    let t0 = base();
    let temps = vec![
        temp(t0 + Duration::seconds(1), 70.0), // 前向流低温 → 报警
        temp(t0 + Duration::seconds(2), 70.0), // 分流期间低温 → 不报警
    ];
    let diverts = vec![
        divert(t0, DivertPosition::Forward),
        divert(t0 + Duration::seconds(2), DivertPosition::Divert),
    ];
    let findings = check_low_temperature(&temps, &diverts, 72.0);
    assert_eq!(findings.len(), 1);
    let (ws, _) = findings[0].window.unwrap();
    assert_eq!(ws, t0 + Duration::seconds(1));
}

#[test]
fn clock_drift_detected_per_channel() {
    let t0 = base();
    let mut s = temp(t0, 74.0);
    s.ingest_time = t0 + Duration::seconds(3); // 时钟差 3s > 1s
    let findings = check_clock_drift(&[s], &[], &[], &spec());
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].kind, FindingKind::ClockDrift);
}

#[test]
fn evidence_gap_detected() {
    let t0 = base();
    let temps = vec![
        temp(t0, 74.0),
        temp(t0 + Duration::seconds(60), 74.0), // 60s 断档 > 10s
    ];
    let findings = check_evidence_gaps(
        t0,
        t0 + Duration::seconds(60),
        &temps,
        &[],
        10.0,
    );
    assert!(kinds(&findings).contains(&FindingKind::EvidenceGap));
}

#[test]
fn passage_window_marks_divert_overlap_uninterpretable() {
    let t0 = base();
    let mut flows = Vec::new();
    for i in 0..=120 {
        flows.push(flow(t0 + Duration::seconds(i), 10_000.0));
    }
    // 分流发生在 40-60s,与 entry=10s 的通过窗(约 43s 停留)重叠
    let diverts = vec![
        divert(t0, DivertPosition::Forward),
        divert(t0 + Duration::seconds(40), DivertPosition::Divert),
        divert(t0 + Duration::seconds(60), DivertPosition::Forward),
    ];
    let windows = reconstruct_passage_windows(
        t0,
        t0 + Duration::seconds(20),
        Duration::seconds(10),
        &flows,
        &diverts,
        120.0,
    );
    assert_eq!(windows.len(), 3);
    assert!(!windows[0].interpretable, "与分流重叠的通过窗必须标记为不可解释");
    assert_eq!(windows[0].obstruction.as_deref(), Some("divert_overlap"));
}

#[test]
fn run_all_aggregates_findings() {
    let t0 = base();
    let mut flows = Vec::new();
    for i in 0..=120 {
        flows.push(flow(t0 + Duration::seconds(i), 10_000.0));
    }
    let temps = vec![temp(t0, 74.0), temp(t0 + Duration::seconds(120), 74.0)];
    let diverts = vec![divert(t0, DivertPosition::Forward)];
    let configs = vec![HoldingTubeConfig {
        id: 1,
        volume_l: 120.0,
        valid_from: t0 - Duration::days(1),
        frozen_by: "e".into(),
        note: None,
    }];
    let input = CheckInput {
        batch_start: t0,
        batch_end: t0 + Duration::seconds(120),
        temps: &temps,
        flows: &flows,
        diverts: &diverts,
        events: &[],
        notices: &[],
        tube_configs: &configs,
        instruments: &[],
        spec: &spec(),
    };
    let findings = run_all(&input);
    // 温度通道在批次内只有 2 个点,间隔 120s > 10s → 必有证据缺口
    assert!(kinds(&findings).contains(&FindingKind::EvidenceGap));
    // 无配置缺失
    assert!(!kinds(&findings).contains(&FindingKind::ConfigMissing));
}

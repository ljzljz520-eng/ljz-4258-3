//! 模拟遥测源:按脚本生成三通道镜像,内置五类故障场景,便于演示与联调。
//! 时间轴(自启动秒数):
//!   180-240  流量突降 10000 → 3000 L/h
//!   300-320  前向流低温事件 74.5 → 70.5 °C
//!   360-380  分流动作,且反馈设备时间戳滞后 5s(反馈迟到)
//!   480-540  停机,流量归零(产品滞留)
//!   600-660  温度通道设备时钟滞后 3s(时钟差)
use super::{Reading, TelemetrySource};
use crate::domain::{DivertPosition, Sample};
use chrono::Utc;

pub struct Simulator {
    tick: u64,
}

impl Simulator {
    pub fn demo() -> Self {
        Self { tick: 0 }
    }

    fn script(elapsed: u64) -> (f64, f64, DivertPosition, i64, i64) {
        // (温度°C, 流量L/h, 分流位置, 温度通道时钟滞后s, 分流通道时钟滞后s)
        let mut temp = 74.5;
        let mut flow = 10_000.0;
        let mut divert = DivertPosition::Forward;
        let mut temp_lag = 0i64;
        let mut divert_lag = 0i64;
        match elapsed {
            180..=240 => flow = 3_000.0,
            300..=320 => temp = 70.5,
            360..=380 => {
                divert = DivertPosition::Divert;
                divert_lag = 5;
            }
            480..=540 => flow = 0.0,
            600..=660 => temp_lag = 3,
            _ => {}
        }
        (temp, flow, divert, temp_lag, divert_lag)
    }
}

impl TelemetrySource for Simulator {
    async fn next_readings(&mut self) -> Vec<Reading> {
        let now = Utc::now();
        let (temp, flow, divert, temp_lag, divert_lag) = Self::script(self.tick);
        self.tick += 1;
        // 微小确定性扰动,避免完全平直
        let wobble = ((self.tick * 7) % 5) as f64 * 0.05;
        vec![
            Reading::Temperature(Sample {
                device_time: now - chrono::Duration::seconds(temp_lag),
                ingest_time: now,
                source: "TT-101".into(),
                value: temp + wobble,
            }),
            Reading::Flow(Sample {
                device_time: now,
                ingest_time: now,
                source: "FT-201".into(),
                value: flow,
            }),
            Reading::Divert(Sample {
                device_time: now - chrono::Duration::seconds(divert_lag),
                ingest_time: now,
                source: "XV-301".into(),
                value: divert,
            }),
        ]
    }
}

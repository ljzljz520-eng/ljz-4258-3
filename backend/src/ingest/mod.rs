//! 遥测接入。安全边界:本模块只定义"读"——
//! 无论是 OPC UA 订阅还是模拟源,都只产生只读镜像,绝不向设备写任何值。
pub mod simulator;
#[cfg(feature = "opcua-live")]
pub mod opcua;

use crate::domain::{DivertPosition, Sample};
use sqlx::PgPool;
use std::time::Duration;
use tracing::{error, info};

/// 一条待入库的遥测(三通道之一)
#[derive(Debug, Clone)]
pub enum Reading {
    Temperature(Sample<f64>),
    Flow(Sample<f64>),
    Divert(Sample<DivertPosition>),
}

/// 遥测源抽象:只能"取下一批读数",接口上不存在写操作。
pub trait TelemetrySource: Send {
    fn next_readings(&mut self) -> impl std::future::Future<Output = Vec<Reading>> + Send;
}

pub async fn run(pool: PgPool) {
    let source_kind = std::env::var("TELEMETRY_SOURCE").unwrap_or_else(|_| "simulator".into());
    info!(source = %source_kind, "遥测接入启动(只读)");
    match source_kind.as_str() {
        #[cfg(feature = "opcua-live")]
        "opcua" => {
            let endpoint = std::env::var("OPCUA_ENDPOINT")
                .unwrap_or_else(|_| "opc.tcp://localhost:4840".into());
            match opcua::ReadOnlyOpcUaSource::connect(&endpoint).await {
                Ok(mut src) => loop_source(&pool, &mut src).await,
                Err(e) => error!(?e, "OPC UA 连接失败,接入退出"),
            }
        }
        _ => {
            let mut src = simulator::Simulator::demo();
            loop_source(&pool, &mut src).await;
        }
    }
}

async fn loop_source<S: TelemetrySource>(pool: &PgPool, src: &mut S) {
    loop {
        for r in src.next_readings().await {
            if let Err(e) = crate::repo::insert_reading(pool, &r).await {
                error!(?e, "遥测入库失败");
            }
        }
        tokio::time::sleep(Duration::from_millis(1000)).await;
    }
}

//! OPC UA 只读接入(特性 `opcua-live`,需要 libssl)。
//! 只读约束的实现方式:会话被包裹在 `ReadOnlySession` 中,
//! 类型上只暴露 subscribe/monitor —— 不存在 write 方法,
//! 即使未来维护者也无法通过此句柄向设备写值。
use super::{Reading, TelemetrySource};
use crate::domain::{DivertPosition, Sample};
use anyhow::{anyhow, Context};
use chrono::Utc;
use opcua::client::prelude::*;
use std::str::FromStr;
use parking_lot::RwLock;
use std::sync::Arc;
use tokio::sync::mpsc;

/// 节点映射:现场位号 → OPC UA NodeId(部署时按现场固定)
const NODE_TEMP: &str = "ns=2;s=HTST.TT101.Temperature";
const NODE_FLOW: &str = "ns=2;s=HTST.FT201.Flow";
const NODE_DIVERT: &str = "ns=2;s=HTST.XV301.DivertPosition";

const HANDLE_TEMP: u32 = 1;
const HANDLE_FLOW: u32 = 2;
const HANDLE_DIVERT: u32 = 3;

/// 只读会话包装:不暴露任何写接口。
struct ReadOnlySession {
    inner: Arc<RwLock<Session>>,
}

impl ReadOnlySession {
    fn subscribe(&self, on_data: mpsc::UnboundedSender<Reading>) -> Result<u32, StatusCode> {
        let session = self.inner.read();
        session.create_subscription(
            1000.0, // 发布间隔 ms
            600,    // lifetime count
            10,     // max keep-alive count
            0,      // max notifications per publish
            0,      // priority
            true,   // publishing enabled
            DataChangeCallback::new(move |items| {
                for item in items {
                    if let Some(r) = map_item(item) {
                        let _ = on_data.send(r);
                    }
                }
            }),
        )
    }

    fn monitor(&self, subscription_id: u32) -> Result<Vec<MonitoredItemCreateResult>, StatusCode> {
        let session = self.inner.read();
        let items = [
            (NODE_TEMP, HANDLE_TEMP),
            (NODE_FLOW, HANDLE_FLOW),
            (NODE_DIVERT, HANDLE_DIVERT),
        ]
        .iter()
        .map(|(nid, handle)| {
            MonitoredItemCreateRequest::new(
                ReadValueId {
                    node_id: NodeId::from_str(nid).unwrap_or_else(|_| NodeId::null()),
                    attribute_id: AttributeId::Value as u32,
                    index_range: UAString::null(),
                    data_encoding: QualifiedName::null(),
                },
                MonitoringMode::Reporting,
                MonitoringParameters {
                    client_handle: *handle,
                    sampling_interval: 1000.0,
                    filter: ExtensionObject::null(),
                    queue_size: 10,
                    discard_oldest: true,
                },
            )
        })
        .collect::<Vec<_>>();
        session.create_monitored_items(subscription_id, TimestampsToReturn::Both, &items)
    }
}

pub struct ReadOnlyOpcUaSource {
    rx: mpsc::UnboundedReceiver<Reading>,
    _session: Arc<RwLock<Session>>, // 保持会话存活;经 ReadOnlySession 只读访问
}

impl ReadOnlyOpcUaSource {
    pub async fn connect(endpoint: &str) -> anyhow::Result<Self> {
        let mut client = ClientBuilder::new()
            .application_name("pv-readonly-mirror")
            .application_uri("urn:pv:readonly-mirror")
            .create_sample_keypair(true)
            .trust_server_certs(true)
            .session_retry_limit(3)
            .client()
            .context("构建 OPC UA 客户端失败")?;

        let session = client
            .connect_to_endpoint(
                (
                    endpoint,
                    SecurityPolicy::None.to_str(),
                    MessageSecurityMode::None,
                    UserTokenPolicy::anonymous(),
                ),
                IdentityToken::Anonymous,
            )
            .map_err(|e| anyhow!("OPC UA 连接失败: {e:?}"))?;

        let (tx, rx) = mpsc::unbounded_channel();
        let ro = ReadOnlySession {
            inner: session.clone(),
        };
        let sub_id = ro
            .subscribe(tx)
            .map_err(|e| anyhow!("创建订阅失败: {e:?}"))?;
        ro.monitor(sub_id)
            .map_err(|e| anyhow!("创建监视项失败: {e:?}"))?;

        Ok(Self {
            rx,
            _session: session,
        })
    }
}

fn map_item(item: &MonitoredItem) -> Option<Reading> {
    let dv = item.last_value();
    let device_time = dv
        .source_timestamp
        .map(|t| t.as_chrono())
        .unwrap_or_else(Utc::now);
    let now = Utc::now();
    let value = dv.value.as_ref()?;
    match item.client_handle() {
        HANDLE_TEMP => Some(Reading::Temperature(Sample {
            device_time,
            ingest_time: now,
            source: "TT-101".into(),
            value: value.as_f64()?,
        })),
        HANDLE_FLOW => Some(Reading::Flow(Sample {
            device_time,
            ingest_time: now,
            source: "FT-201".into(),
            value: value.as_f64()?,
        })),
        HANDLE_DIVERT => {
            let pos = match value {
                Variant::Boolean(b) => {
                    if *b {
                        DivertPosition::Divert
                    } else {
                        DivertPosition::Forward
                    }
                }
                other => match other.as_f64() {
                    Some(x) if x >= 0.5 => DivertPosition::Divert,
                    _ => DivertPosition::Forward,
                },
            };
            Some(Reading::Divert(Sample {
                device_time,
                ingest_time: now,
                source: "XV-301".into(),
                value: pos,
            }))
        }
        _ => None,
    }
}

impl TelemetrySource for ReadOnlyOpcUaSource {
    async fn next_readings(&mut self) -> Vec<Reading> {
        let mut out = Vec::new();
        while let Ok(r) = self.rx.try_recv() {
            out.push(r);
        }
        out
    }
}

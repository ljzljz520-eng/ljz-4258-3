# 乳品巴氏杀菌验证平台

把**流量、加热温度、保持管容积、分流状态**与**产品批**对应起来:传感器位置、
设备时钟与流向变化共同决定一段热历程是否可解释。平台只做**只读镜像 + 验证比对**,
不输出杀菌设定值,不控制分流阀。

## 架构

```
┌────────────┐  只读订阅   ┌──────────────────────┐   ┌────────────────────┐
│  OPC UA    │ ─────────▶ │ Axum 后端 (Rust)      │   │ TimescaleDB        │
│  (只读会话) │            │ ingest → repo → api   │──▶│ 遥测超表(设备时钟) │
└────────────┘            │ analysis: 纯函数检查  │   │ 批次/配置/事件/发现 │
                          └─────────┬────────────┘   └────────────────────┘
                                    │ REST
                          ┌─────────▼────────────┐
                          │ Svelte 前端           │
                          │ 通过窗重建/时间线/发现 │
                          └──────────────────────┘
```

### 安全边界(由类型系统与 API 面强制)

| 边界 | 实现 |
|---|---|
| OPC UA 只读 | `ReadOnlySession` 包装只暴露 subscribe/monitor,类型上不存在 write |
| 不提供杀菌设定 | 低温限值等阈值全部由验证工程师**冻结输入**(`validation_specs`),平台只比对 |
| 不控制分流阀 | 分流状态仅为只读镜像;API 无任何阀门端点 |
| 操作员权限 | 仅能标记**换料/循环/停机/取样**四类事件(`EventKind` 枚举封闭) |
| 工程师权限 | 冻结保持管容积、仪表版本、验证规格(`x-role: engineer` 守卫) |

### 时钟模型

每条遥测同时保存 `device_time`(设备时钟/源时间戳)与 `ingest_time`(平台接收时钟);
TimescaleDB 超表按 **device_time** 分区 —— 热历程以设备时钟为准,
两钟之差(时钟差)本身是被检查对象。

## 检查项与测试场景映射

| 测试场景 | 检查 | 严重级 |
|---|---|---|
| 温度探头校准后标签未更新 | `check_instrument_label`:校准通知后无新冻结版本,或新旧版本标签相同 | critical |
| 流量突降 | `check_flow_drop`:低于批次中位流量 × 冻结比例;同时拉长通过窗停留时间 | warning |
| 分流反馈迟到 | `check_divert_feedback_delay`:分流样本 ingest−device 超阈 | critical |
| 保持管改造仍用旧容积 | `check_holding_volume`:改造通知后无新冻结容积,通过窗结论无效 | critical |
| 停机时产品滞留 | `check_product_held` + 容积积分遇停流即判 `HeldInTube` | critical |
| — | 低温事件(仅前向流期间判定,分流期间属预期) | critical |
| — | 时钟差(按通道聚合最差样本) | warning |
| — | 证据缺口(遥测断档) | warning |
| — | 通过窗与分流状态重叠 → 不可解释 | (通过窗标记) |

### 通过窗重建(容积积分法)

出口时刻 = 从进入时刻起,对实际流量阶梯积分,累计体积达到**冻结容积**之时。
- 积分路径遇**实测停流** → `HeldInTube`(即使流量恢复,也不再是已验证的连续流状态);
- "无数据"与"实测零流量"严格区分(前者是证据问题,后者才是停流);
- 通过窗与分流状态重叠 → 标记不可解释。

## 运行

```bash
# 1. TimescaleDB(生产);普通 PostgreSQL 也可运行(自动退化为常规表)
docker compose up -d

# 2. 后端(默认模拟遥测源;真实接入见下)
cd backend
DATABASE_URL=postgres://pv:pv@localhost:5432/pasteurization cargo run

# 2b. 真实 OPC UA 只读接入(需要 libssl)
cargo run --features opcua-live
# TELEMETRY_SOURCE=opcua OPCUA_ENDPOINT=opc.tcp://plc:4840

# 3. 前端
cd frontend && npm install && npm run dev   # http://localhost:5173

# 4. 测试(12 个场景/单元测试,纯函数,无需数据库)
cd backend && cargo test
```

模拟遥测源时间轴(自启动秒数):180–240 流量突降 → 300–320 前向流低温 →
360–380 分流+反馈迟到 5s → 480–540 停机滞留 → 600–660 温度通道时钟滞后 3s。

## API 摘要

| 方法 | 路径 | 角色 | 说明 |
|---|---|---|---|
| POST | `/api/batches` | 任意 | 开批 |
| POST | `/api/batches/{id}/close` | 任意 | 关批 |
| POST | `/api/batches/{id}/events` | operator+ | 标记换料/循环/停机/取样 |
| POST | `/api/configs/holding-tube` | engineer | 冻结保持管容积 |
| POST | `/api/configs/instruments` | engineer | 冻结仪表版本(标签+校准日期) |
| POST | `/api/configs/spec` | engineer | 冻结验证规格(限值/阈值) |
| POST | `/api/notices` | 任意 | 录入工程通知(校准/改造事实) |
| POST | `/api/batches/{id}/analyze` | 任意 | 运行全部检查,落库发现 |
| GET | `/api/batches/{id}/timeline` | 任意 | 时间线(遥测+事件+发现) |
| GET | `/api/batches/{id}/passage?entry=…` | 任意 | 单点通过窗重建 |
| GET | `/api/batches/{id}/findings` | 任意 | 发现列表 |

角色通过请求头 `x-role: operator|engineer`、`x-user: <工号>` 传递(演示用;
生产应替换为真实 IAM)。

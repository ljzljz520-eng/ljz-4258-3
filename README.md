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
                          │ 通过窗/时间线/发现/证据审核 │
                          └──────────────────────┘
```

### 证据归属(批次 × 安装位置)

每条遥测样本在审核界面与 `/api/batches/{id}/evidence` 中都显式携带:**所属产品批**
(id + 产品名)、**通道**、**位号**与**安装位置**(`sensor_position` 登记:TT-101→保持管出口、
FT-201→保持管入口、XV-301→分流阀;未登记位号显式标注"未登记位置",不留空)。
每条验证发现的 `detail.evidence` 同样给出通道/位号/安装位置 —— 热历程证据的来源边界
可逐条核对,不依赖隐式约定。

### 冻结输入校验

开批与冻结配置在**前端提交前**与**后端落库前**双重校验(后端返回 400):
产品名非空、容积为有限正数(含 NaN 拒绝)、仪表位号/标签非空、
规格各时间阈值为有限正数、流量突降比例 ∈ (0, 1]。

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

### 再循环产品链

分流阀把产品切回平衡罐时,这段"回流料"并未离开热历程 —— 它会再次进入保持管。
平台按分流状态翻转点把批次切成交替的**前向段/回流段**:**每次回到平衡罐
(回流段结束)就形成一次新的通过尝试**;最终去向(灌装机)的热暴露 =
链上**全部**通过尝试的并集,**不得只显示最后一次合格段** —— 任一通过缺
温度证据都必须显式标注,而不是被最后一次合格段掩盖。

物料平衡(库存结算,段内线性保守近似):回流段体积计入"平衡罐未走完回流料"
库存,前向段体积从中扣减(先走回流料,库存不为负)。该近似只用于判定
"事件发生时回流料是否仍未走完",不用于计算停留时间。

| 测试场景 | 检查 | 严重级 |
|---|---|---|
| 回流量未计 | `check_chain`:回流段内无流量样本 → 物料平衡不闭合 | critical |
| 两批在平衡罐混合 | `check_chain`:换料时罐内仍有未走完回流料 | critical |
| 再循环跨清洗边界 | `check_chain`:清洗开始时回流料未走完,或清洗期间仍在回流 | critical |
| 第一次通过记录缺温度 | `check_chain`:第一次通过无温度样本,该段热暴露无法验证 | critical |
| 最终产品只取部分回流料 | `check_chain`:链尾仍有未走完回流料,去向台账须保留 | warning |

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

# 4. 测试(27 个场景/单元测试,纯函数,无需数据库)
cd backend && cargo test
```

模拟遥测源时间轴(自启动秒数):180–240 流量突降 → 300–320 前向流低温 →
360–380 分流+反馈迟到 5s → 480–540 停机滞留 → 600–660 温度通道时钟滞后 3s →
700–725 再次分流(再循环第二段)。

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
| GET | `/api/batches/{id}/timeline` | 任意 | 时间线(遥测+事件+发现+传感器来源) |
| GET | `/api/batches/{id}/evidence?channel=…` | 任意 | 审核证据清单(逐条:批次归属+安装位置) |
| GET | `/api/batches/{id}/passage?entry=…` | 任意 | 单点通过窗重建 |
| GET | `/api/batches/{id}/recirculation` | 任意 | 再循环产品链(通过尝试+物料平衡+链级发现) |
| GET | `/api/batches/{id}/findings` | 任意 | 发现列表 |

角色通过请求头 `x-role: operator|engineer`、`x-user: <工号>` 传递(演示用;
生产应替换为真实 IAM)。

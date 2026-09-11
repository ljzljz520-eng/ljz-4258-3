-- 巴氏杀菌验证平台:遥测镜像(按设备时钟分区)+ 批次/配置/事件/发现
-- TimescaleDB 可用时启用超表;普通 PostgreSQL 下退化为常规表(便于本地开发/测试)
DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM pg_available_extensions WHERE name = 'timescaledb') THEN
        CREATE EXTENSION IF NOT EXISTS timescaledb;
    END IF;
END $$;

-- ============ 遥测镜像(只读写入,设备时钟为分区键) ============
CREATE TABLE IF NOT EXISTS telemetry_temperature (
    device_time timestamptz NOT NULL,          -- 设备时钟(源时间戳)
    ingest_time timestamptz NOT NULL DEFAULT now(), -- 平台接收时钟
    sensor_id   text NOT NULL,
    value_c     double precision NOT NULL,
    quality     text NOT NULL DEFAULT 'Good'
);
DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM pg_proc WHERE proname = 'create_hypertable') THEN
        PERFORM create_hypertable('telemetry_temperature', 'device_time', if_not_exists => TRUE);
    END IF;
END $$;
CREATE INDEX IF NOT EXISTS idx_temp_sensor_time ON telemetry_temperature (sensor_id, device_time DESC);

CREATE TABLE IF NOT EXISTS telemetry_flow (
    device_time timestamptz NOT NULL,
    ingest_time timestamptz NOT NULL DEFAULT now(),
    meter_id    text NOT NULL,
    value_lph   double precision NOT NULL,
    quality     text NOT NULL DEFAULT 'Good'
);
DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM pg_proc WHERE proname = 'create_hypertable') THEN
        PERFORM create_hypertable('telemetry_flow', 'device_time', if_not_exists => TRUE);
    END IF;
END $$;
CREATE INDEX IF NOT EXISTS idx_flow_meter_time ON telemetry_flow (meter_id, device_time DESC);

CREATE TABLE IF NOT EXISTS telemetry_divert (
    device_time timestamptz NOT NULL,
    ingest_time timestamptz NOT NULL DEFAULT now(),
    valve_id    text NOT NULL,
    position    text NOT NULL,                 -- forward | divert
    quality     text NOT NULL DEFAULT 'Good'
);
DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM pg_proc WHERE proname = 'create_hypertable') THEN
        PERFORM create_hypertable('telemetry_divert', 'device_time', if_not_exists => TRUE);
    END IF;
END $$;
CREATE INDEX IF NOT EXISTS idx_divert_valve_time ON telemetry_divert (valve_id, device_time DESC);

-- ============ 产品批 ============
CREATE TABLE IF NOT EXISTS batches (
    id           bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    product_name text NOT NULL,
    started_at   timestamptz NOT NULL DEFAULT now(),
    ended_at     timestamptz,
    status       text NOT NULL DEFAULT 'active'   -- active | closed
);

-- ============ 操作员事件(仅四类:换料/循环/停机/取样) ============
CREATE TABLE IF NOT EXISTS operator_events (
    id        bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    batch_id  bigint NOT NULL REFERENCES batches(id),
    kind      text NOT NULL,                       -- changeover | cycle | shutdown | sample
    at        timestamptz NOT NULL,
    note      text,
    marked_by text NOT NULL DEFAULT 'operator'
);
CREATE INDEX IF NOT EXISTS idx_events_batch ON operator_events (batch_id, at);

-- ============ 验证工程师冻结的配置(版本化,平台不生成设定值) ============
CREATE TABLE IF NOT EXISTS holding_tube_configs (
    id         bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    volume_l   double precision NOT NULL CHECK (volume_l > 0),
    valid_from timestamptz NOT NULL DEFAULT now(),
    frozen_by  text NOT NULL,
    note       text
);

CREATE TABLE IF NOT EXISTS instrument_versions (
    id            bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    instrument_id text NOT NULL,
    label         text NOT NULL,                   -- 现场标签/位号版本
    calibrated_at timestamptz NOT NULL,
    valid_from    timestamptz NOT NULL DEFAULT now(),
    frozen_by     text NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_instr ON instrument_versions (instrument_id, valid_from);

-- 验证规格:低温限值与各项阈值,全部由工程师冻结输入;平台只比对,不计算设定值
CREATE TABLE IF NOT EXISTS validation_specs (
    id                    bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    min_hold_temp_c       double precision NOT NULL,
    max_clock_skew_s      double precision NOT NULL DEFAULT 1.0,
    max_gap_s             double precision NOT NULL DEFAULT 10.0,
    max_divert_feedback_s double precision NOT NULL DEFAULT 2.0,
    flow_drop_ratio       double precision NOT NULL DEFAULT 0.6,
    valid_from            timestamptz NOT NULL DEFAULT now(),
    frozen_by             text NOT NULL,
    note                  text
);

-- ============ 工程通知(维护系统侧事实:校准、改造) ============
CREATE TABLE IF NOT EXISTS engineering_notices (
    id            bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    kind          text NOT NULL,               -- probe_recalibrated | tube_modified
    instrument_id text,
    reported_at   timestamptz NOT NULL,
    message       text NOT NULL DEFAULT ''
);

-- ============ 验证发现 ============
CREATE TABLE IF NOT EXISTS findings (
    id           bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    batch_id     bigint NOT NULL REFERENCES batches(id),
    kind         text NOT NULL,
    severity     text NOT NULL,
    window_start timestamptz,
    window_end   timestamptz,
    message      text NOT NULL,
    detail       jsonb NOT NULL DEFAULT '{}',
    detected_at  timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_findings_batch ON findings (batch_id);

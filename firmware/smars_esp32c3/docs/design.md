# SMARS ESP32-C3 主控固件设计文档

状态：评审中（疑问见 §10，未定项以 *标注）
工具链：Rust + esp-idf-svc（std, `riscv32imc-esp-espidf`），沿用本机 hexapod 项目既有栈
关联：STC8H1K08 侧协议见 `../../stc8h1k08_pwm/docs/design.md`（协议 v2，本固件为主机端实现）

---

## 1. 功能概述

- ESP32-C3 为 SMARS 小车主控：WiFi softAP + **网页遥控器**（手机/PC 直连热点访问）。
- 经 UART0（115200 8N1）与 STC8H1K08 通信（协议 v2），获得 8 路 PWM：驱动 2 路电机组（差速）与可选升降电机。
- 板载外设：2×SG90 舵机（LEDC PWM）、HC-SR04 超声波（可选）、1S 锂电池电压检测、蜂鸣器、WS2812 状态灯。
- 5 种 3D 打印件配置，**Rust feature 切换**编译出对应固件；每台 SMARS 有名字（网页标题/mDNS/热点名）。
- 安全一等功：网页失联、串口异常、上电默认 → 电机停止。

## 2. 硬件引脚（ESP32-C3）

| IO | 功能 | 说明 |
|---|---|---|
| IO0 / IO1 | SDA / SCL | I2C 总线，本期预留（不初始化） |
| IO2 | BUZZ | 无源蜂鸣器（LEDC 音频 PWM）。⚠ IO2 为 strapping 脚，驱动电路复位时不得拉高 |
| IO3 | BAT_ADC | 1S 锂电池（分压后输入），ADC 单次采样+滑动平均 |
| IO4 / IO5 | PWM9 / PWM10 | SG90 舵机 1 / 舵机 2（LEDC 50 Hz，500~2500 µs） |
| IO6 / IO7 | TRIG / ECHO | HC-SR04。⚠ ECHO 为 5V，硬件须分压/电平转换 |
| IO10 | WS2812 | 单状态灯；`ws2812` 特性为灯带（`config::WS2812_COUNT` 颗，默认 20，改常量即可） |
| IO20 / IO21 | UART0 RX / TX | ↔ STC8H1K08 P3.7(TXD)/P3.6(RXD)，交叉连接；日志走 USB-Serial-JTAG |

上电默认：所有输出停止态（PWM 停、舵机保持、蜂鸣器静）。

## 3. 机器人配置矩阵

| # | feature 配置名 | 底盘 | 舵机（IO4/IO5） | 超声波 | 升降（STC PWM→RZ7888） | 网页额外 UI |
|---|---|---|---|---|---|---|
| 1 | `config-basic` | 左右 2 电机组差速 | 无 | 固定安装*（Q2） | 无 | 距离显示 |
| 2 | `config-gripper` | 同上 | 上下 0~90° + 夹持 0~90° | 无 | 无 | 双舵机滑条 |
| 3 | `config-loader` | 同上 | 上下 0~60° | 无 | 无 | 单舵机滑条 |
| 4 | `config-forklift` | 同上 | 上下 0~60° | 无 | N20 正反转 | 舵机滑条 + 升/降按住按钮 |
| 5 | `config-sonar` | 同上 | 云台 ±60°（居中 90°） | 随云台转动 | 无 | 云台滑条 + 距离显示 |

所有配置公共 UI：电池电压、前进/后退/左转/右转（触屏按钮 + 键盘 WASD/方向键）、急停、链路状态。

### 3.1 可选外设特性（与 robot-* 叠加）

| feature | 行为 | 网页 UI |
|---|---|---|
| `buzz` | 开机播放 R2-D2 风格旋律（参考 servo-robot `startup_sound.rs`，独立线程经 LEDC `tone(freq,ms)` 播放） | 喇叭按钮（按下发"滴滴"） |
| `ws2812` | IO10 灯带 `WS2812_COUNT`（默认 20，`config.rs` 一处改）；效果引擎在 `smars-core::leds`（纯逻辑可单测） | 灯带开关、亮度滑条、效果按钮（常亮/呼吸/彩虹/流水） |

未启用时的行为：`buzz` 未启用开机短促一声；`ws2812` 未启用 IO10 为单状态灯（电池/急停/链路色）。启用 `ws2812` 后状态色不抢灯，由效果引擎独占灯带。

## 4. STC8H1K08 通道映射*（Q3 待确认）

差速底盘每侧为 1 路电机组（可并联 2 个电机）。建议默认接线：

| STC 通道 | 信号 | 接到 |
|---|---|---|
| CH0 / CH1 | 左电机组 IN1 / IN2 | DRV8833 #1 通道 A（1 路 PWM + 1 路方向/PWM） |
| CH2 / CH3 | 右电机组 IN1 / IN2 | DRV8833 #1 通道 B |
| CH4 / CH5 | 电机 3 IN1/IN2 或 RZ7888 INA/INB | DRV8833 #2 通道 A（4 电机配置）或升降（叉车配置） |
| CH6 / CH7 | 电机 4 IN1/IN2 | DRV8833 #2 通道 B（4 电机配置）；其余配置备用 |

运动控制（每侧）：前进 = IN1 占空比 $v$、IN2=0；后退 = IN1=0、IN2=$v$；停止 = 双 0（滑行）；
$v = \mathrm{round}(速度 \times 10000)$，协议 duty 0~10000。频率统一 A 组/B 组各 20 kHz（可配）。
混控：$v_L = v_{前进} - v_{转向}$，$v_R = v_{前进} + v_{转向}$，饱和后归一化。

## 5. 软件架构

```
firmware/smars_esp32c3/
├── Cargo.toml              # workspace
├── crates/
│   ├── smars-protocol/     # 纯 Rust：STC 协议 v2 编解码（CRC8/帧/命令/应答/回读），host 可测
│   └── smars-esp32c3/      # 固件 bin（harness = false）
│       ├── src/
│       │   ├── main.rs     # 装配：WiFi + HTTP + 任务
│       │   ├── config.rs   # feature 选择：NAME/配置矩阵/舵机限位/电池系数（compile_error! 保证唯一）
│       │   ├── wifi.rs     # AP/STA 模式切换 + NVS 存储 WiFi 凭据 + STA 失败回落 AP
│       │   ├── stc_link.rs # UART0 + 请求/应答 + 超时重试 + 链路状态
│       │   ├── drive.rs    # 差速混控 + 斜坡 + 失联停车
│       │   ├── servo.rs    # LEDC 50Hz 角度↔脉宽（含上电缓动）
│       │   ├── sonar.rs    # HC-SR04 定时测量（可选编译）
│       │   ├── battery.rs  # ADC 采样 + 分压系数 + 滤波 + 电量估算
│       │   ├── ui.rs       # 状态灯/蜂鸣器反馈
│       │   └── web.rs      # EspHttpServer：GET /、POST /cmd、GET /status
│       ├── static/         # 遥控网页（内嵌 include_str!，按配置编译不同 UI）
│       └── .cargo/config.toml / sdkconfig.defaults
└── docs/design.md  README.md
```

- 复用 hexapod 项目既有模式：softAP + `EspHttpServer` + `include_str!` 网页 + 纯协议 crate + `cargo test` host 单测。
- 并发模型（std 线程）：主循环/HTTP 各自处理；`stc_link` 独立线程持 UART 与命令队列；共享状态 `Arc<Mutex<State>>`。
- STC 交互：设置命令发后等 ACK（超时 20 ms ×3 重试）；`0x06` 查询用于上电确认与排障；急停用 `0x04`，恢复 `0x05`。

## 6. 网页遥控器

- 单页 vanilla HTML/JS（无框架），按 `config` 编译期拼入对应 UI 段（§3 最右列）。
- 驾驶：按住期间每 200 ms 发送当前方向（松开立即发"停止"）；键盘 WASD/方向键同源。
- 遥测：`GET /status` 每 500 ms 轮询——电池电压/电量%、距离 cm（含超时状态）、舵机当前角度、链路/急停状态。
- 急停按钮（始终可见）→ `POST /cmd {"estop":1}`；恢复按钮。
- 页面标题与状态栏显示机器人名字（§8）。
- **WiFi 设置区**（AP+STA 可切换）：显示当前模式/IP；STA 模式下填 SSID/密码（存 NVS，重启后自动连接）；
  连接失败 15 s 回落 AP 热点（网页提示"STA 失败，已回落 AP"）；可一键清除 STA 配置。
- IP 发现：AP 模式固定 `http://192.168.4.1/`；STA 模式 IP 由路由器分配（页面状态栏/路由器列表查看）。
  mDNS（`smars-<name>.local`）未实现——mdns 是 ESP-IDF 托管组件，esp-idf-sys 默认构建不含该组件，
  收益（省一次查路由器）不抵组件接入成本；如需可后补 `ESP_IDF_COMPONENTS` 方案。

## 7. 安全设计

| 触发 | 动作 |
|---|---|
| 上电 | 电机全停、STC 上电默认恒低，网页未连接不可动 |
| 网页失联（>1 s 无 cmd/心跳） | 立即停车（滑行） |
| STC 无应答/链路断 | 重试 3 次失败 → 网页红字告警 + 停车 |
| 急停 | `0x04` 锁存（STC 侧恒低），网页明确显示"已急停"，需手动恢复 |
| 升降按钮 | 按住式（松开即停），单次按压超时 3 s 自动停 |

## 8. 命名与 feature 方案（已定：方案 A）

- 每台一个 feature `robot-<name>`，隐含配置；`NAME` 常量编译进固件（热点名 `SMARS-<NAME>`、网页标题同名）。
- 占位名（后可改）与配置对应：

| feature | 名字 | 配置 |
|---|---|---|
| `robot-bolt` | bolt | config-basic |
| `robot-fork` | fork | config-gripper |
| `robot-cargo` | cargo | config-loader |
| `robot-lift` | lift | config-forklift |
| `robot-scan` | scan | config-sonar |

- `compile_error!` 保证恰好启用一个机器人 feature。

## 9. 构建与提交计划

### 9.1 工具链约束（实现中实测，重要）

| 约束 | 处理 |
|---|---|
| esp-idf-sys 0.38 与 ESP-IDF 5.4+ 的 newlib `_reent` 布局不兼容；esp-idf-svc 0.53 引用 5.4+ 的 `n_report` 字段 | 钉 **ESP-IDF 5.3.2**（`.cargo/config.toml` 的 `ESP_IDF_VERSION`），并对 vendored `lib/esp-idf-svc` 补一行 `report` 字段名补丁（`[patch.crates-io]`） |
| 最新 nightly 在 esp-idf-svc 0.53 上触发 rustc ICE | 钉 **nightly-2025-06-01**（`rust-toolchain.toml`） |
| embuild 0.33 的 cmake 路径不输出 `--ldproxy-linker`，ldproxy 0.3.5 强制要求该参数 | 应用 crate 的 `build.rs` 调 `embuild::espidf::sysenv::output()` 转发链接参数（esp-idf-template 标准写法），并兜底注入 `--ldproxy-linker` |
| `links` 元数据传播的链接参数（libnewlib/esp-idf 组件库）必须由应用 crate 转发才作用于最终链接 | 同上，`build.rs` 转发 |
| 目标 `riscv32imc-esp-espidf` 为 tier-3 无预编译 std | nightly + `build-std`（`.cargo/config.toml`） |

WS2812 用 `rmt-legacy` API（新 RMT API 需自定义编码器，收益低）；一次性 ADC 用 `adc-oneshot-legacy`。两者均为有意的弃用告警。

### 9.2 网页验证记录（浏览器实测）

按配置注入 `window.SMARS_CONFIG` 后逐项验证（Chromium 加载，JS 全量执行无异常）：

| 配置 | 舵机滑条 | 超声波 | 升降 | 其他 |
|---|---|---|---|---|
| bolt（basic） | 无 | ✓ | 无 | 驾驶盘/急停/WiFi 区齐全 |
| fork（gripper） | 2 条（0~90 上下、0~90 夹持） | 无 | 无 | 同上 |
| lift（forklift） | 1 条（0~60） | 无 | ✓ | 同上 |
| scan（sonar turret） | 1 条（-60~60 云台） | ✓ | 无 | 同上 |

离线容错：无 /status 服务时链路徽标显示"离线"，页面不崩溃。

可选特性 UI（浏览器实测）：`buzz` 喇叭按钮按 CFG.hasBuzz 显隐、按下无异常；
`ws2812` 灯带面板（20 颗标注、开关、亮度、4 效果按钮）按 CFG.hasLight 显隐，
效果/开关/亮度交互无异常；未启用特性时两面板正确隐藏。

```bash
cargo test                                             # host：smars-protocol + 纯逻辑单测
cd crates/smars_esp32c3
cargo build --release --features robot-<name>           # .cargo/config.toml 已定 riscv32imc-esp-espidf
cargo run  --release --features robot-<name>            # espflash 烧录 + 监视
```

阶段提交：
1. 本文档（设计 + 疑问）
2. workspace 骨架 + `smars-protocol`（协议 v2 编解码 + host 单测）
3. 机器人模型层（混控/舵机角度/电池换算，纯逻辑 + 单测）
4. 固件集成（WiFi AP + HTTP + UART + 安全停机，config-basic 跑通）
5. 网页遥控器（全部配置 UI + feature 切换）
6. 全 feature 构建矩阵验证 + 文档定稿

## 10. 已确认决策（2026-10-12 评审）

| 问题 | 决策 |
|---|---|
| Q1 WiFi 模式 | **AP+STA 可切换**：默认 AP 热点 `SMARS-<NAME>`；网页可配 STA（凭据存 NVS），失败 15 s 回落 AP |
| Q2 配置 1 超声波 | **固定安装，只显示距离**；云台转动仅配置 5 |
| Q3 STC 通道映射 | **按 §4 建议表**：CH0/1 左电机组、CH2/3 右电机组、CH4/5=第二块 DRV8833 A 或 RZ7888 升降、CH6/7=B/备用 |
| Q4 命名与 feature | **方案 A**：每台一个 feature，占位名 bolt/fork/cargo/lift/scan（§8） |
| Q5 电池分压 | **2:1，系数 2.0**，软件留校准微调位 |

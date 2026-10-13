# SMARS ESP32-C3 主控固件

SMARS 小车主控：WiFi 网页遥控器 + STC8H1K08（8 路 PWM，协议 v2）+ SG90 舵机 + 超声波 + 电池检测。
Rust（esp-idf-svc / ESP-IDF 5.3.2），按机器人配置以 Cargo feature 切换。

- 设计文档（配置矩阵、协议、安全设计、工具链约束、验证记录）：[docs/design.md](docs/design.md)
- STC 侧协议：[`../stc8h1k08_pwm/docs/design.md`](../stc8h1k08_pwm/docs/design.md)

## 机器人配置（feature）

| feature | 名字 | 配置 | 网页额外 UI |
|---|---|---|---|
| `robot-bolt` | bolt | 基础（超声波固定） | 距离显示 |
| `robot-fork` | fork | 夹持（2×SG90） | 上下 0~90 + 夹持 0~90 |
| `robot-cargo` | cargo | 装载（1×SG90） | 上下 0~60 |
| `robot-lift` | lift | 叉车（SG90 + N20 升降） | 上下 0~60 + 升/降按钮 |
| `robot-scan` | scan | 超声波云台（SG90 ±60°） | 云台滑条 + 距离显示 |
| `robot-truck` | truck | 卡车（4 电机 + 车斗 2×SG90 联动 + 超声波） | 车斗滑条（0~90°）+ 距离显示 |

`robot-*` feature 必须恰好启用一个（编译期断言）。

可选外设特性（与 robot-* 叠加）：

| feature | 行为 |
|---|---|
| `buzz` | 开机播放 R2-D2 风格启动音；网页喇叭按钮（"滴滴"）。**未启用 = 硬件不焊蜂鸣器，固件不初始化、无声** |
| `ws2812` | IO10 灯带（`config::WS2812_COUNT`，默认 20）；网页开关/亮度/效果（常亮、呼吸、彩虹、流水） |

## 一次性准备

```bash
cargo install ldproxy espflash      # 链接代理 / 烧录
espup install --targets esp32c3     # RISC-V 目标（GitHub API 限流时：GITHUB_TOKEN=$(gh auth token) espup install …）
```

工具链版本已钉死（见 `crates/smars_esp32c3/rust-toolchain.toml` 与 `.cargo/config.toml` 注释）：
nightly-2025-06-01 + ESP-IDF 5.3.2，首次构建自动下载 ESP-IDF 与交叉工具链（约 15 分钟）。

## 构建 / 烧录

```bash
# host 单测（协议编解码 + 机器人模型纯逻辑）
cargo test

# 固件（在 crates/smars_esp32c3 目录下执行，.cargo/config.toml 按目录生效）
cd crates/smars_esp32c3
cargo build --release --features robot-bolt
cargo run   --release --features robot-bolt     # espflash 烧录 + 监视（P3.0/P3.1 ISP）
```

STC8H1K08 的系统时钟在其 ISP 下载时设定（stcgal `-t 22118` = 22.1184 MHz，与其固件
`__CONF_FOSC` 一致，见 STC 侧文档）；ESP32-C3 侧串口固定 115200 8N1（IO20/IO21 ↔ STC P3.7/P3.6）。

## 使用

1. 上电后连接热点 `SMARS-<名字>`（密码 `smars1234`），访问 `http://192.168.4.1/`；
   或在网页 WiFi 设置区填家庭 WiFi（存 NVS，重启后 STA 连接，失败 15 s 回落热点）。
2. 网页：电池电压、驾驶（触屏/键盘 WASD、按住 200 ms 心跳、松开即停）、
   按配置显示舵机/超声波/升降 UI、急停（再点解除）、WiFi 设置。
3. 安全：网页失联 1 s 停车、升降单次按压 3 s 上限、急停经 STC 协议锁存（`0x04`/`0x05`）。

## 验证状态

已验证：host 单测 16 项全过；5 个 robot feature 全部构建通过；网页 UI 按配置浏览器实测（docs §9.2）。
尚未验证：目标硬件实测（无硬件环境）——上电联调清单见 docs §9 与 `../stc8h1k08_pwm/docs/design.md` §9。

## 目录

```
crates/smars-protocol/   STC 协议 v2 编解码（纯 Rust，host 可测）
crates/smars-core/       机器人模型：混控/舵机映射/电池/超声波换算（纯逻辑，host 可测）
crates/smars_esp32c3/    固件（WiFi + 网页遥控 + UART + 外设）
lib/esp-idf-svc/         vendored 0.53（单行 ESP-IDF 5.3 字段名补丁，[patch.crates-io]）
docs/                    设计文档
```

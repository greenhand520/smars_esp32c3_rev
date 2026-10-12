# SMARS ESP32-C3 主控固件

SMARS 小车主控：WiFi 网页遥控器 + STC8H1K08（8 路 PWM）+ SG90 舵机 + 超声波 + 电池检测。
Rust（esp-idf-svc），按机器人配置以 Cargo feature 切换。

设计文档（含配置矩阵、协议、待确认问题）：[docs/design.md](docs/design.md)

## 构建

```bash
cargo test                                   # host 单测（smars-protocol）
cd crates/smars_esp32c3
cargo build --release --features robot-<name>
cargo run   --release --features robot-<name> # espflash 烧录 + 监视
```

## 目录

```
crates/smars-protocol/   STC 协议 v2 编解码（纯 Rust，host 可测）
crates/smars_esp32c3/    固件（WiFi AP + 网页遥控 + UART + 外设）
docs/                    设计文档
```

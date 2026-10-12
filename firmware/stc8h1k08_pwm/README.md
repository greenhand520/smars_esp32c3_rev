# STC8H1K08 八路 PWM 固件

STC8H1K08 通过 UART1 接收主控 MCU 命令，输出 8 路 PWM（PWMA×4 + PWMB×4）。
设计、通信协议 v2 与待确认问题见 [docs/design.md](docs/design.md)。

## 构建

```bash
pio run                      # 编译（SDCC, intel_mcs51 平台）
pio run -t size              # 代码/RAM 占用
pio run -t upload            # stcgal 烧录（P3.0/P3.1 ISP）
```

系统时钟在 ISP 下载时设定（stcgal `-t <kHz>`），固件编译宏 `__CONF_FOSC` 必须与之一致；
波特率默认 115200 8N1（UART1：P3.6/RXD、P3.7/TXD）。

## 测试

```bash
make -C test/host test       # 协议层主机端单元测试（gcc）
```

## 目录

```
src/          固件源码（main / uart_io / protocol / pwm_ctrl）
lib/          FwLib_STC8（vendored, Apache-2.0）
test/host/    协议解析与 CRC8 主机端单元测试
docs/         设计文档
```

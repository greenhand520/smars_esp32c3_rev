# STC8H1K08 八路 PWM 固件

STC8H1K08 通过 UART1 接收主控 MCU 命令，输出 8 路 PWM（PWMA×4 + PWMB×4，2 路独立频率、8 路独立占空比）。

- 设计、通信协议 v2、硬件约束与上电验证清单：[docs/design.md](docs/design.md)
- 数据手册：`/mnt/MyFiles/Hardware/datasheet/STC-STC8H1K08-36I-QFN20.pdf`

## 硬件接口

| 功能 | 引脚 | 说明 |
|---|---|---|
| CH0..CH3 | P1.0 / P1.2 / P1.4 / P1.6 | PWM1P..PWM4P（PWMA） |
| CH4..CH7 | P1.7 / P5.4 / P3.3 / P3.4 | PWM5..PWM8（PWMB，`PWMB_PS=0x55`） |
| UART1 RXD/TXD | P3.6 / P3.7 | 115200 8N1，接主控 |
| ISP | P3.0 / P3.1 | 仅下载用 |
| LED 初始化指示 | P1.1 | 初始化完成后恒高 |
| LED PWM 已配置指示 | P1.3 | 设置命令成功后高，急停后低 |

上电默认 8 路输出恒低。协议命令：`0x01` 全量设置（2 组频率+8 占空比）、`0x02` 单通道占空比、
`0x03` 全量占空比、`0x04` 急停（锁存）、`0x05` 恢复、`0x06` 状态查询、`0x07` 版本查询；
每命令均有 ACK/NACK 应答。帧格式与 CRC8 测试向量见 docs/design.md §4。

## 构建与烧录

```bash
pio run                      # 编译（SDCC, intel_mcs51 平台）
pio run -t size              # 代码/RAM 占用
pio run -t upload            # stcgal 烧录（P3.0/P3.1 ISP）
```

- **系统时钟由 ISP 下载时设定**：stcgal `-t 22118`（= board_build.f_cpu）自动微调 IRC 至
  22.1184 MHz；固件不写 IRC 微调寄存器。换时钟频率须同步改 `platformio.ini` 的
  `board_build.f_cpu` 与 `__CONF_FOSC`（两处必须一致）。
- ISP 选项注意：P5.4 必须保持普通 I/O（设为 RST 复位脚会废掉 PWM6）。

## 测试

```bash
make -C test/host test       # 协议解析 + PWM 换算主机端单元测试（gcc）
```

已验证：主机端单测全部通过、固件编译通过（Flash 6843/8192 B、XRAM 219/1024 B）。
尚未验证：目标硬件实测（无硬件环境），上电步骤按 docs/design.md §9 清单执行。

## 目录

```
src/          main（命令分发）/ uart_io（收发+1ms 时间基）/ protocol（帧+CRC8）/ pwm_ctrl / pwm_math
lib/          FwLib_STC8（vendored，Apache-2.0，上游 fw-lib_-stc8 @9dce329）
              —— srcFilter 仅编译 fw_uart.c/fw_util.c，其余头文件宏直接使用
test/host/    主机端单元测试（test_protocol、test_pwm_math）
docs/         设计文档
```

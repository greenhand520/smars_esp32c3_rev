/*
 * UART1 收发与 1ms 时间基。
 *
 * UART1 引脚切换到 P3.6/RXD、P3.7/TXD（数据手册 §3.1.1，S1_S = 01）；
 * P3.0/P3.1 保留给 ISP 下载。接收经环形缓冲由主循环消费，发送阻塞。
 * 时间基：Timer0 每 1ms 中断一次（tick_ms，8 位循环计数，帧超时判断用）。
 */
#ifndef UART_IO_H
#define UART_IO_H

#include <stdint.h>

extern volatile uint8_t tick_ms;

void uart_io_init(void);
void tick_init(void);

/* 取一个收到的字节：1 = 成功，0 = 缓冲空 */
uint8_t uart_io_read(uint8_t *b);

/* 阻塞发送 */
void uart_io_write(const uint8_t *buf, uint8_t len);

#endif

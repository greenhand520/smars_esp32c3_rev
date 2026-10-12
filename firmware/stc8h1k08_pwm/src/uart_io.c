#include "fw_hal.h"
#include "uart_io.h"

#define RX_RING_SIZE 64
#define RX_RING_MASK (RX_RING_SIZE - 1)

/* 环形缓冲放 XDATA，节省仅 128B 的 DATA 区 */
static __xdata uint8_t rx_ring[RX_RING_SIZE];
static volatile uint8_t rx_head;
static volatile uint8_t rx_tail;
volatile uint8_t tick_ms;

void tick_init(void)
{
    /*
     * Timer0：1T、16 位自动重载（模式 0，数据手册 §2 特性），每 1ms 中断一次。
     * 用寄存器宏直接配置（fw_tim.c 的辅助函数会把未用到的 Timer3/4 配置函数
     * 一并链入，撑爆仅 128B 的 DATA 区）。
     */
    uint16_t reload = (uint16_t)(65536UL - (__SYSCLOCK / 1000UL));

    TIM_Timer0_Set1TMode(HAL_State_ON);
    TIM_Timer0_SetMode(TIM_TimerMode_16BitAuto);
    TIM_Timer0_SetInitValue((uint8_t)(reload >> 8), (uint8_t)reload);
    ET0 = 1;
    EA = 1;
    TIM_Timer0_SetRunState(HAL_State_ON);
}

void uart_io_init(void)
{
    rx_head = 0;
    rx_tail = 0;

    /* P3.7/TXD 推挽输出，P3.6/RXD 高阻输入 */
    GPIO_P3_SetMode(GPIO_Pin_6, GPIO_Mode_Input_HIP);
    GPIO_P3_SetMode(GPIO_Pin_7, GPIO_Mode_Output_PP);

    /* 8N1，波特率源 Timer2 1T 模式 */
    UART1_Config8bitUart(UART1_BaudSource_Timer2, HAL_State_ON, 115200);
    UART1_SwitchPort(UART1_AlterPort_P36_P37);
    UART1_SetRxState(HAL_State_ON);
    ES = 1;
    EA = 1;
}

uint8_t uart_io_read(uint8_t *b)
{
    if (rx_tail == rx_head)
        return 0;
    *b = rx_ring[rx_tail];
    rx_tail = (uint8_t)((rx_tail + 1) & RX_RING_MASK);
    return 1;
}

void uart_io_write(const uint8_t *buf, uint8_t len)
{
    uint8_t i;
    for (i = 0; i < len; i++)
    {
        UART1_WriteBuffer(buf[i]);
        while (!TI)
            ;
        UART1_ClearTxInterrupt();
    }
}

/* 串口 1 中断：收字节入环形缓冲（缓冲满则丢弃，靠帧超时/重同步恢复） */
INTERRUPT(UART1_Isr, 4)
{
    if (RI)
    {
        UART1_ClearRxInterrupt();
        rx_ring[rx_head] = SBUF;
        if ((uint8_t)((rx_head + 1) & RX_RING_MASK) != rx_tail)
            rx_head = (uint8_t)((rx_head + 1) & RX_RING_MASK);
    }
    if (TI)
    {
        UART1_ClearTxInterrupt();
    }
}

/* Timer0 中断：1ms 时间基 */
INTERRUPT(Timer0_Isr, 1)
{
    tick_ms++;
}

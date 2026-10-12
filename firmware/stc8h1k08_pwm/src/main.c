/*
 * STC8H1K08 八路 PWM 控制器固件
 *
 * 通信协议：docs/design.md
 */
#include "fw_hal.h"

void main(void)
{
    /*
     * 时钟初始化：SYSCLK = MCLK（IRC 频率在 ISP 下载时设定）。
     * CLKDIV 复位值为 4（SYSCLK = MCLK/4，数据手册 §6.2.4），此处置 0 取全速。
     * 故意不调用 SYS_SetClock()：它会重写 IRC 微调寄存器
     * （IRCBAND/VRTRIM/IRTRIM/LIRTRIM），数据手册 §6.2 明确警告不调整频率时
     * 不能随意修改这些寄存器，否则运行频率会变。
     */
    SFRX_ON();
    CLKDIV = 0x00;
    SFRX_OFF();

    /* P1.1：初始化完成指示；P1.3：PWM 已配置指示。
     * 复位后 P1.x 为高阻输入（数据手册 §2.1.2），须显式配置为推挽输出。 */
    GPIO_P1_SetMode(GPIO_Pin_1 | GPIO_Pin_3, GPIO_Mode_Output_PP);
    P13 = 0;
    P11 = 1;

    while (1)
    {
    }
}

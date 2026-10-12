/*
 * STC8H1K08 8-channel PWM controller firmware
 *
 * Protocol: docs/design.md
 */
#include "fw_hal.h"

void main(void)
{
    /*
     * Clock init: SYSCLK = MCLK (IRC trimmed at ISP upload time).
     * CLKDIV reset value is 4 (SYSCLK = MCLK/4, datasheet 6.2.4), set it to 0.
     * Deliberately NOT calling SYS_SetClock(): it rewrites the IRC trim
     * registers (IRCBAND/VRTRIM/IRTRIM/LIRTRIM), which datasheet 6.2 forbids
     * unless the firmware itself changes the frequency.
     */
    SFRX_ON();
    CLKDIV = 0x00;
    SFRX_OFF();

    /* P1.1: init-done LED; P1.3: PWM-configured LED. P1.x are high-Z after
     * reset (datasheet 2.1.2), so set push-pull output explicitly. */
    GPIO_P1_SetMode(GPIO_Pin_1 | GPIO_Pin_3, GPIO_Mode_Output_PP);
    P13 = 0;
    P11 = 1;

    while (1)
    {
    }
}

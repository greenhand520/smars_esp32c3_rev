/*
 * PWM 换算数学（纯 C，无硬件依赖，供 pwm_ctrl.c 与主机端单测共用）。
 *
 * 边沿对齐频率公式（数据手册 §20.7.19）：
 *     f = SYSCLK / ((PSCR+1) * (ARR+1))
 * 占空比步进为 1/(ARR+1)，协议 0..10000 按四舍五入量化到计数步。
 */
#ifndef PWM_MATH_H
#define PWM_MATH_H

#include <stdint.h>

#define PWM_MATH_DUTY_MAX 10000u

/*
 * 目标频率 freq（Hz，1..65535）→ (PSCR, ARR)。
 * 尽量取最大 ARR（即最细的占空比分辨率）：ARR+1 ≤ 65535，
 * 超出时用 PSCR 粗分频。实际频率 = sysclk / ((PSCR+1)*(ARR+1))。
 */
static void pwm_math_freq(uint32_t sysclk, uint16_t freq,
                          uint16_t *pscr, uint16_t *arr)
{
    uint32_t total, psc, n;

    total = sysclk / freq;
    if (total <= 65535UL)
    {
        psc = 0;
        n = total;
    }
    else
    {
        /* psc = PSCR+1 = ceil(total / 65535)，保证 n ≤ 65535 */
        psc = (total + 65534UL) / 65535UL;
        n = total / psc;
    }
    if (n < 2)
        n = 2;
    *pscr = (uint16_t)(psc ? psc - 1UL : 0UL);
    *arr = (uint16_t)(n - 1UL);
}

/*
 * 占空比 duty（0..10000，对应 0.00%..100.00%）→ CCR 计数值（四舍五入）。
 * duty == 10000 时返回 ARR+1，LowIfLess 模式下输出恒高；duty == 0 返回 0，恒低。
 */
static uint16_t pwm_math_ccr(uint16_t duty, uint16_t arr)
{
    uint32_t n = (uint32_t)arr + 1UL;
    return (uint16_t)((duty * n + 5000UL) / 10000UL);
}

/*
 * CCR 计数值 → 实际输出占空比（0..10000，四舍五入）。
 * 量化回读用：实际步进 = 1/(ARR+1)，与 pwm_math_ccr 构成往返。
 */
static uint16_t pwm_math_actual_duty(uint16_t ccr, uint16_t arr)
{
    uint32_t n = (uint32_t)arr + 1UL;
    return (uint16_t)((ccr * 10000UL + n / 2UL) / n);
}

#endif

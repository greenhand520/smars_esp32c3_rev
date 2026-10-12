/*
 * pwm_math.h 主机端单元测试（gcc 编译）。
 *
 * 覆盖：频率→(PSCR,ARR) 换算的取值范围与实际频率误差、
 *       占空比→CCR 的量化边界与单调性。
 */
#include <stdio.h>
#include "../../src/pwm_math.h"

#define SYSCLK 22118400UL /* 与 __CONF_FOSC 默认值一致 */

static int failures = 0;

#define CHECK(cond) do { \
    if (!(cond)) { \
        printf("FAIL %s:%d: %s\n", __FILE__, __LINE__, #cond); \
        failures++; \
    } \
} while (0)

/* 实际频率 = sysclk / ((PSCR+1)*(ARR+1))，以千分比返回误差 */
static int32_t freq_err_permille(uint16_t f, uint16_t pscr, uint16_t arr)
{
    uint32_t period = ((uint32_t)pscr + 1UL) * ((uint32_t)arr + 1UL);
    uint32_t fact = SYSCLK / period;
    int32_t diff = (int32_t)fact - (int32_t)f;

    return diff * 1000 / (int32_t)f;
}

static void test_known_cases(void)
{
    uint16_t pscr, arr;

    /* 50 Hz：total = 442368 > 65535，走粗分频 */
    pwm_math_freq(SYSCLK, 50, &pscr, &arr);
    CHECK(pscr == 6 && arr == 63194);

    /* 65535 Hz：total = 337，PSCR = 0 */
    pwm_math_freq(SYSCLK, 65535, &pscr, &arr);
    CHECK(pscr == 0 && arr == 336);

    /* 1 Hz：PSCR 接近 total/65535 */
    pwm_math_freq(SYSCLK, 1, &pscr, &arr);
    CHECK(pscr == 337 && arr == 65438);

    /* 20 kHz：total = 1105，PSCR = 0 */
    pwm_math_freq(SYSCLK, 20000, &pscr, &arr);
    CHECK(pscr == 0 && arr == 1104);
}

static void test_freq_sweep(void)
{
    uint32_t f;

    for (f = 1; f <= 65535UL; f += 13)
    {
        uint16_t pscr, arr;
        int32_t err;

        pwm_math_freq(SYSCLK, (uint16_t)f, &pscr, &arr);
        /* 取值范围 */
        CHECK((uint32_t)arr + 1UL <= 65535UL);
        CHECK(arr >= 1);
        /* 实际频率误差 ≤ 1%（量化 + 取整） */
        err = freq_err_permille((uint16_t)f, pscr, arr);
        CHECK(err >= -10 && err <= 10);
    }
}

static void test_ccr(void)
{
    /* 边界 */
    CHECK(pwm_math_ccr(0, 9999) == 0);
    CHECK(pwm_math_ccr(10000, 9999) == 10000);   /* = ARR+1，恒高 */
    CHECK(pwm_math_ccr(10000, 336) == 337);      /* = ARR+1，恒高 */
    CHECK(pwm_math_ccr(5000, 9999) == 5000);
    CHECK(pwm_math_ccr(5000, 336) == 169);       /* round(0.5*337) = 168.5 → 169 */

    /* 单调不减 + 不超过 ARR+1 */
    {
        uint32_t arr;
        for (arr = 2; arr <= 65535UL; arr += 97)
        {
            uint16_t prev = 0, d;
            for (d = 0; d <= PWM_MATH_DUTY_MAX; d += 137)
            {
                uint16_t ccr = pwm_math_ccr(d, (uint16_t)arr);
                CHECK(ccr >= prev);
                CHECK((uint32_t)ccr <= arr + 1UL);
                prev = ccr;
            }
            CHECK(pwm_math_ccr(PWM_MATH_DUTY_MAX, (uint16_t)arr) == (uint16_t)(arr + 1UL));
        }
    }
}

int main(void)
{
    test_known_cases();
    test_freq_sweep();
    test_ccr();

    if (failures)
    {
        printf("%d check(s) FAILED\n", failures);
        return 1;
    }
    printf("all pwm math tests passed\n");
    return 0;
}

#include "fw_hal.h"
#include "pwm_math.h"
#include "pwm_ctrl.h"

#define GROUP_A 0
#define GROUP_B 1

/* 软触发更新事件（UG）：把预装载的 PSCR/ARR/CCRx 立即加载生效 */
#define PWM_UG_A()  SFRX_SET(PWMA_EGR, 0)
#define PWM_UG_B()  SFRX_SET(PWMB_EGR, 0)

/* 大数组放 XDATA，避免 SDCC 局部变量静态叠放占满仅 128B 的 DATA 区 */
static __xdata uint16_t req_freq[2];    /* 请求频率（Hz），0 = 该组恒低 */
static __xdata uint16_t req_duty[PWM_CH_COUNT]; /* 请求占空比 0..10000 */
static __xdata uint16_t cur_arr[2];     /* 换算得到的 ARR（占空比量化用） */
static __xdata uint8_t  out_pwm[PWM_CH_COUNT];  /* 1 = 输出 PWM，0 = 强制恒低 */
static uint8_t  estop_latched;
static uint8_t  configured;

/* 逐路设置输出模式：PWM_LowIfLess（CCRx 定占空比）/ AlwaysLow（强制恒低） */
static void set_mode(uint8_t ch, uint8_t pwm_on)
{
    switch (ch)
    {
    case 0: PWMA_PWM1_ConfigOutputMode(pwm_on ? PWM_OutputMode_PWM_LowIfLess : PWM_OutputMode_AlwaysLow); break;
    case 1: PWMA_PWM2_ConfigOutputMode(pwm_on ? PWM_OutputMode_PWM_LowIfLess : PWM_OutputMode_AlwaysLow); break;
    case 2: PWMA_PWM3_ConfigOutputMode(pwm_on ? PWM_OutputMode_PWM_LowIfLess : PWM_OutputMode_AlwaysLow); break;
    case 3: PWMA_PWM4_ConfigOutputMode(pwm_on ? PWM_OutputMode_PWM_LowIfLess : PWM_OutputMode_AlwaysLow); break;
    case 4: PWMB_PWM1_ConfigOutputMode(pwm_on ? PWM_OutputMode_PWM_LowIfLess : PWM_OutputMode_AlwaysLow); break;
    case 5: PWMB_PWM2_ConfigOutputMode(pwm_on ? PWM_OutputMode_PWM_LowIfLess : PWM_OutputMode_AlwaysLow); break;
    case 6: PWMB_PWM3_ConfigOutputMode(pwm_on ? PWM_OutputMode_PWM_LowIfLess : PWM_OutputMode_AlwaysLow); break;
    case 7: PWMB_PWM4_ConfigOutputMode(pwm_on ? PWM_OutputMode_PWM_LowIfLess : PWM_OutputMode_AlwaysLow); break;
    }
}

/* 逐路写占空比计数值（预装载，周期边界生效） */
static void set_ccr(uint8_t ch, uint16_t ccr)
{
    switch (ch)
    {
    case 0: PWMA_PWM1_SetCaptureCompareValue(ccr); break;
    case 1: PWMA_PWM2_SetCaptureCompareValue(ccr); break;
    case 2: PWMA_PWM3_SetCaptureCompareValue(ccr); break;
    case 3: PWMA_PWM4_SetCaptureCompareValue(ccr); break;
    case 4: PWMB_PWM1_SetCaptureCompareValue(ccr); break;
    case 5: PWMB_PWM2_SetCaptureCompareValue(ccr); break;
    case 6: PWMB_PWM3_SetCaptureCompareValue(ccr); break;
    case 7: PWMB_PWM4_SetCaptureCompareValue(ccr); break;
    }
}

/* 应用单路：急停或该组频率为 0 时强制恒低，否则按请求占空比量化 */
static void apply_channel(uint8_t ch)
{
    uint8_t pwm_on = (!estop_latched) && (req_freq[ch >> 2] != 0);

    if (pwm_on != out_pwm[ch])
    {
        set_mode(ch, pwm_on);
        out_pwm[ch] = pwm_on;
    }
    if (pwm_on)
        set_ccr(ch, pwm_math_ccr(req_duty[ch], cur_arr[ch >> 2]));
}

/*
 * 应用组频率（freq == 0 时保持 PSCR/ARR 不动，输出由模式强制恒低）。
 * PSCR/ARR 为预装载寄存器，需 UG 才生效，由调用方决定何时触发。
 */
static void apply_group_freq(uint8_t group, uint16_t freq)
{
    uint16_t pscr, arr;

    if (freq == 0)
        return;
    pwm_math_freq((uint32_t)__SYSCLOCK, freq, &pscr, &arr);
    cur_arr[group] = arr;
    if (group == GROUP_A)
    {
        PWMA_SetPrescaler(pscr);
        PWMA_SetPeriod(arr);
    }
    else
    {
        PWMB_SetPrescaler(pscr);
        PWMB_SetPeriod(arr);
    }
}

void pwm_init(void)
{
    uint8_t ch;

    /* 8 路 PWM 引脚推挽输出（复位后为高阻输入，数据手册 §2.1.2） */
    GPIO_P1_SetMode(GPIO_Pin_0 | GPIO_Pin_2 | GPIO_Pin_4 | GPIO_Pin_6 | GPIO_Pin_7,
                    GPIO_Mode_Output_PP);
    GPIO_P3_SetMode(GPIO_Pin_3 | GPIO_Pin_4, GPIO_Mode_Output_PP);
    GPIO_P5_SetMode(GPIO_Pin_4, GPIO_Mode_Output_PP);

    /* 引脚切换：PWMA 用默认组（P1.0/P1.2/P1.4/P1.6），PWMB 切到 P1.7/P5.4/P3.3/P3.4 */
    PWMA_PWM1_SetPort(PWMA_PWM1_AlterPort_P10_P11);
    PWMA_PWM2_SetPort(PWMA_PWM2_AlterPort_P12P54_P13);
    PWMA_PWM3_SetPort(PWMA_PWM3_AlterPort_P14_P15);
    PWMA_PWM4_SetPort(PWMA_PWM4_AlterPort_P16_P17);
    PWMB_PWM1_SetPort(PWMB_PWM5_AlterPort_P17);
    PWMB_PWM2_SetPort(PWMB_PWM6_AlterPort_P54);
    PWMB_PWM3_SetPort(PWMB_PWM7_AlterPort_P33);
    PWMB_PWM4_SetPort(PWMB_PWM8_AlterPort_P34);

    for (ch = 0; ch < PWM_CH_COUNT; ch++)
    {
        switch (ch)
        {
        case 0:
            PWMA_PWM1_SetPortState(HAL_State_OFF);
            PWMA_PWM1_SetPortDirection(PWMA_PortDirOut);
            PWMA_PWM1_SetComparePreload(HAL_State_ON);
            break;
        case 1:
            PWMA_PWM2_SetPortState(HAL_State_OFF);
            PWMA_PWM2_SetPortDirection(PWMA_PortDirOut);
            PWMA_PWM2_SetComparePreload(HAL_State_ON);
            break;
        case 2:
            PWMA_PWM3_SetPortState(HAL_State_OFF);
            PWMA_PWM3_SetPortDirection(PWMA_PortDirOut);
            PWMA_PWM3_SetComparePreload(HAL_State_ON);
            break;
        case 3:
            PWMA_PWM4_SetPortState(HAL_State_OFF);
            PWMA_PWM4_SetPortDirection(PWMA_PortDirOut);
            PWMA_PWM4_SetComparePreload(HAL_State_ON);
            break;
        case 4:
            PWMB_PWM1_SetPortState(HAL_State_OFF);
            PWMB_PWM1_SetPortDirection(PWMB_PortDirOut);
            PWMB_PWM1_SetComparePreload(HAL_State_ON);
            break;
        case 5:
            PWMB_PWM2_SetPortState(HAL_State_OFF);
            PWMB_PWM2_SetPortDirection(PWMB_PortDirOut);
            PWMB_PWM2_SetComparePreload(HAL_State_ON);
            break;
        case 6:
            PWMB_PWM3_SetPortState(HAL_State_OFF);
            PWMB_PWM3_SetPortDirection(PWMB_PortDirOut);
            PWMB_PWM3_SetComparePreload(HAL_State_ON);
            break;
        case 7:
            PWMB_PWM4_SetPortState(HAL_State_OFF);
            PWMB_PWM4_SetPortDirection(PWMB_PortDirOut);
            PWMB_PWM4_SetComparePreload(HAL_State_ON);
            break;
        }
    }
    /* 上电安全态：8 路全部强制恒低 */
    for (ch = 0; ch < PWM_CH_COUNT; ch++)
    {
        set_mode(ch, 0);
        out_pwm[ch] = 0;
    }

    /* ARR 预装载开启，占空比/周期批量更新在周期边界原子生效 */
    PWMA_SetAutoReloadPreload(HAL_State_ON);
    PWMB_SetAutoReloadPreload(HAL_State_ON);
    PWMA_SetEdgeAlignment(PWM_EdgeAlignment_Side);
    PWMB_SetEdgeAlignment(PWM_EdgeAlignment_Side);
    PWMA_SetCounterDirection(PWM_CounterDirection_Up);
    PWMB_SetCounterDirection(PWM_CounterDirection_Up);
    PWMA_SetPrescaler(0);
    PWMB_SetPrescaler(0);
    PWMA_SetPeriod(0xFFFF);
    PWMB_SetPeriod(0xFFFF);

    /* 正端输出使能（互补输出 PWMxN 不用，保持禁用以免占用 P1.1/P1.3/P1.7） */
    PWMA_SetPinOutputState(PWM_Pin_1 | PWM_Pin_2 | PWM_Pin_3 | PWM_Pin_4, HAL_State_ON);
    PWMB_SetPinOutputState(PWMB_Pin_All, HAL_State_ON);

    PWMA_SetOverallState(HAL_State_ON);
    PWMB_SetOverallState(HAL_State_ON);
    PWMA_SetCounterState(HAL_State_ON);
    PWMB_SetCounterState(HAL_State_ON);

    req_freq[GROUP_A] = 0;
    req_freq[GROUP_B] = 0;
    for (ch = 0; ch < PWM_CH_COUNT; ch++)
        req_duty[ch] = 0;
    cur_arr[GROUP_A] = 0;
    cur_arr[GROUP_B] = 0;
    estop_latched = 0;
    configured = 0;
}

uint8_t pwm_set_all(uint16_t freq_a, uint16_t freq_b, const uint16_t *duty)
{
    uint8_t ch, ug_a = 0, ug_b = 0;

    for (ch = 0; ch < PWM_CH_COUNT; ch++)
        if (duty[ch] > PWM_DUTY_MAX)
            return 1;
    for (ch = 0; ch < PWM_CH_COUNT; ch++)
        req_duty[ch] = duty[ch];

    /*
     * 频率不变的组不写 PSCR/ARR、不触发 UG：
     * 占空比经预装载在周期边界平滑生效，不撕裂当前周期。
     */
    if (freq_a != req_freq[GROUP_A])
    {
        req_freq[GROUP_A] = freq_a;
        apply_group_freq(GROUP_A, freq_a);
        ug_a = 1;
    }
    if (freq_b != req_freq[GROUP_B])
    {
        req_freq[GROUP_B] = freq_b;
        apply_group_freq(GROUP_B, freq_b);
        ug_b = 1;
    }
    for (ch = 0; ch < PWM_CH_COUNT; ch++)
        apply_channel(ch);
    if (ug_a)
        PWM_UG_A();
    if (ug_b)
        PWM_UG_B();

    configured = 1;
    return 0;
}

uint8_t pwm_set_duty(uint8_t ch, uint16_t duty)
{
    if (ch >= PWM_CH_COUNT || duty > PWM_DUTY_MAX)
        return 1;
    req_duty[ch] = duty;
    apply_channel(ch);
    return 0;
}

uint8_t pwm_set_duties(const uint16_t *duty)
{
    uint8_t ch;

    for (ch = 0; ch < PWM_CH_COUNT; ch++)
        if (duty[ch] > PWM_DUTY_MAX)
            return 1;
    for (ch = 0; ch < PWM_CH_COUNT; ch++)
    {
        req_duty[ch] = duty[ch];
        apply_channel(ch);
    }
    return 0;
}

void pwm_estop(void)
{
    uint8_t ch;

    estop_latched = 1;
    for (ch = 0; ch < PWM_CH_COUNT; ch++)
        apply_channel(ch); /* 强制恒低 */
}

void pwm_resume(void)
{
    uint8_t ch;

    estop_latched = 0;
    for (ch = 0; ch < PWM_CH_COUNT; ch++)
        apply_channel(ch); /* 按保存的配置还原 */
}

void pwm_get_state(pwm_state_t *out)
{
    uint8_t ch;

    out->freq_a = req_freq[GROUP_A];
    out->freq_b = req_freq[GROUP_B];
    for (ch = 0; ch < PWM_CH_COUNT; ch++)
        out->duty[ch] = req_duty[ch];
}

uint8_t pwm_estop_active(void)
{
    return estop_latched;
}

uint8_t pwm_configured(void)
{
    return configured;
}

/*
 * PWM 控制层：组频率/占空比应用、急停/恢复、状态回读。
 *
 * 通道映射（见 docs/design.md §2）：
 *   CH0..CH3 = PWM1P..PWM4P（PWMA，共享 PSCR/ARR）
 *   CH4..CH7 = PWM5..PWM8  （PWMB，共享 PSCR/ARR）
 * 每路占空比独立（CCRx）。
 */
#ifndef PWM_CTRL_H
#define PWM_CTRL_H

#include <stdint.h>

#define PWM_CH_COUNT 8
#define PWM_DUTY_MAX 10000u

typedef struct
{
    uint16_t freq_a;                  /* A 组实际输出频率 Hz（向下取整），0 = 停 */
    uint16_t freq_b;                  /* B 组实际输出频率 Hz（向下取整），0 = 停 */
    uint16_t n_a;                     /* A 组每周期计数步 ARR+1（占空比步进 1/n_a），0 = 未配置 */
    uint16_t n_b;                     /* B 组每周期计数步 ARR+1（占空比步进 1/n_b），0 = 未配置 */
    uint16_t duty[PWM_CH_COUNT];      /* 实际输出占空比 0..10000（量化后） */
} pwm_actual_t;

/* 初始化引脚与 PWM 定时器，上电默认 8 路恒低 */
void pwm_init(void);

/* 参数合法返回 0；占空比 > 10000 返回 1 */
uint8_t pwm_set_all(uint16_t freq_a, uint16_t freq_b, const uint16_t *duty);
uint8_t pwm_set_duty(uint8_t ch, uint16_t duty);
uint8_t pwm_set_duties(const uint16_t *duty);

/* 急停：8 路强制恒低并锁存；恢复：解除锁存并还原上次配置 */
void pwm_estop(void);
void pwm_resume(void);

/* 回读实际生效值（含量化后占空比、每组分辨率、实际频率） */
void pwm_get_actual(pwm_actual_t *out);
uint8_t pwm_estop_active(void);
uint8_t pwm_configured(void);

#endif

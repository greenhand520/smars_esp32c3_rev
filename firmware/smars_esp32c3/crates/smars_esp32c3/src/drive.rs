//! 差速驱动 → STC PWM 输出映射（docs/design.md §4）。
//!
//! 通道分配（固定接线）：
//! CH0/1 = 左电机组 IN1/IN2，CH2/3 = 右电机组 IN1/IN2，CH4/5 = 升降 RZ7888 INA/INB，
//! CH6/7 = 备用。两组频率统一 20 kHz。

use smars_core::{mix_diff, throttle_to_pins};
use smars_protocol::Command;

/// STC 通道索引
const CH_L_IN1: usize = 0;
const CH_L_IN2: usize = 1;
const CH_R_IN1: usize = 2;
const CH_R_IN2: usize = 3;
const CH_X_IN1: usize = 4;
const CH_X_IN2: usize = 5;

/// PWM 频率（Hz）：20 kHz，超出听觉范围，电机驱动友好
pub const FREQ_HZ: u16 = 20_000;

/// 前进/转向（-10000..=10000）+ 升降油门（同量纲）→ 全量设置命令。
/// 备用通道恒 0。
pub fn set_all_command(forward: i32, turn: i32, lift: i32) -> Command {
    let (left, right) = mix_diff(forward, turn);
    let (l1, l2) = throttle_to_pins(left);
    let (r1, r2) = throttle_to_pins(right);
    let (x1, x2) = throttle_to_pins(lift);

    let mut duties = [0u16; 8];
    duties[CH_L_IN1] = l1;
    duties[CH_L_IN2] = l2;
    duties[CH_R_IN1] = r1;
    duties[CH_R_IN2] = r2;
    duties[CH_X_IN1] = x1;
    duties[CH_X_IN2] = x2;
    Command::SetAll { freq_a: FREQ_HZ, freq_b: FREQ_HZ, duties }
}

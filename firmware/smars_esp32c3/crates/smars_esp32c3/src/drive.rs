//! 差速驱动 → STC PWM 输出映射（docs/design.md §4）。
//!
//! 两种布局（由 `config::DriveLayout` 决定）：
//! - 双电机组：CH0/1 = 左 IN1/IN2，CH2/3 = 右 IN1/IN2，CH4/5 = 升降 RZ7888，CH6/7 备用；
//! - 四电机（2×DRV8833 用满 8 通道）：左 = CH0/1 + CH4/5，右 = CH2/3 + CH6/7。
//! 两组频率统一 20 kHz。

use smars_core::{mix_diff, throttle_to_pins};
use smars_protocol::Command;

use crate::config::DriveLayout;

/// PWM 频率（Hz）：20 kHz，超出听觉范围，电机驱动友好
pub const FREQ_HZ: u16 = 20_000;

/// 前进/转向（-10000..=10000）+ 升降油门（同量纲，仅双电机组布局使用）
/// → 全量设置命令。
pub fn set_all_command(forward: i32, turn: i32, lift: i32, layout: DriveLayout) -> Command {
    let (left, right) = mix_diff(forward, turn);
    let (l1, l2) = throttle_to_pins(left);
    let (r1, r2) = throttle_to_pins(right);

    let mut duties = [0u16; 8];
    match layout {
        DriveLayout::DualGroup => {
            duties[0] = l1;
            duties[1] = l2;
            duties[2] = r1;
            duties[3] = r2;
            let (x1, x2) = throttle_to_pins(lift);
            duties[4] = x1;
            duties[5] = x2;
        }
        DriveLayout::QuadMotor => {
            // 左前/左后（CH0/1、CH4/5），右前/右后（CH2/3、CH6/7）
            duties[0] = l1;
            duties[1] = l2;
            duties[4] = l1;
            duties[5] = l2;
            duties[2] = r1;
            duties[3] = r2;
            duties[6] = r1;
            duties[7] = r2;
        }
    }
    Command::SetAll { freq_a: FREQ_HZ, freq_b: FREQ_HZ, duties }
}

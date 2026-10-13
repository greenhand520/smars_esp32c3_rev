//! 机器人身份与配置矩阵：Cargo feature → 名字/舵机/外设配置。
//!
//! 见 `docs/design.md` §3（配置矩阵）与 §8（命名方案 A）。
//! `robot-*` feature 必须恰好启用一个（编译期断言）。

use smars_core::ServoSpec;

/// 舵机描述：网页 UI + 通道绑定（1 = IO4/PWM9，2 = IO5/PWM10）
#[derive(Debug, Clone, Copy)]
pub struct ServoUi {
    pub id: u8,
    pub label: &'static str,
    pub spec: ServoSpec,
}

/// 机器人配置（由 feature 决定）
#[derive(Debug, Clone, Copy)]
pub struct RobotConfig {
    /// 名字：热点 `SMARS-<name>`、mDNS `smars-<name>.local`、网页标题
    pub name: &'static str,
    pub servos: &'static [ServoUi],
    /// 是否接超声波（显示距离）
    pub has_sonar: bool,
    /// 是否有升降电机（RZ7888，STC CH4/CH5）
    pub has_lift: bool,
    /// BAT_ADC 分压系数（2:1 分压 = 2000）
    pub battery_scale_permille: u32,
}

/// 常用舵机规格（逻辑域 → 物理域，见 smars-core::ServoSpec）
const SERVO_UP_DOWN_0_90: ServoSpec =
    ServoSpec { logical_min: 0, logical_max: 90, physical_min: 0, physical_max: 90 };
const SERVO_UP_DOWN_0_60: ServoSpec =
    ServoSpec { logical_min: 0, logical_max: 60, physical_min: 0, physical_max: 60 };
const SERVO_GRIPPER_0_90: ServoSpec =
    ServoSpec { logical_min: 0, logical_max: 90, physical_min: 0, physical_max: 90 };
const SERVO_TURRET_PM60: ServoSpec =
    ServoSpec { logical_min: -60, logical_max: 60, physical_min: 30, physical_max: 150 };

const BATTERY_SCALE_2_TO_1: u32 = 2000;

/// 恰好启用一个 robot-* feature 的编译期断言
const SELECTED_ROBOTS: u32 = cfg!(feature = "robot-bolt") as u32
    + cfg!(feature = "robot-fork") as u32
    + cfg!(feature = "robot-cargo") as u32
    + cfg!(feature = "robot-lift") as u32
    + cfg!(feature = "robot-scan") as u32;
const _: () = assert!(SELECTED_ROBOTS == 1, "必须恰好启用一个 robot-* feature");

#[cfg(feature = "robot-bolt")]
pub const CONFIG: RobotConfig = RobotConfig {
    name: "bolt",
    servos: &[],
    has_sonar: true,
    has_lift: false,
    battery_scale_permille: BATTERY_SCALE_2_TO_1,
};

#[cfg(feature = "robot-fork")]
pub const CONFIG: RobotConfig = RobotConfig {
    name: "fork",
    servos: &[
        ServoUi { id: 1, label: "上下", spec: SERVO_UP_DOWN_0_90 },
        ServoUi { id: 2, label: "夹持", spec: SERVO_GRIPPER_0_90 },
    ],
    has_sonar: false,
    has_lift: false,
    battery_scale_permille: BATTERY_SCALE_2_TO_1,
};

#[cfg(feature = "robot-cargo")]
pub const CONFIG: RobotConfig = RobotConfig {
    name: "cargo",
    servos: &[ServoUi { id: 1, label: "上下", spec: SERVO_UP_DOWN_0_60 }],
    has_sonar: false,
    has_lift: false,
    battery_scale_permille: BATTERY_SCALE_2_TO_1,
};

#[cfg(feature = "robot-lift")]
pub const CONFIG: RobotConfig = RobotConfig {
    name: "lift",
    servos: &[ServoUi { id: 1, label: "上下", spec: SERVO_UP_DOWN_0_60 }],
    has_sonar: false,
    has_lift: true,
    battery_scale_permille: BATTERY_SCALE_2_TO_1,
};

#[cfg(feature = "robot-scan")]
pub const CONFIG: RobotConfig = RobotConfig {
    name: "scan",
    servos: &[ServoUi { id: 1, label: "云台", spec: SERVO_TURRET_PM60 }],
    has_sonar: true,
    has_lift: false,
    battery_scale_permille: BATTERY_SCALE_2_TO_1,
};

/// WS2812 灯带灯数（`ws2812` 特性；改这里即可适配不同长度灯带）
pub const WS2812_COUNT: usize = 20;

/// 可选外设特性（编译期，与 robot-* 叠加）
pub const HAS_BUZZ: bool = cfg!(feature = "buzz");
pub const HAS_LIGHT: bool = cfg!(feature = "ws2812");

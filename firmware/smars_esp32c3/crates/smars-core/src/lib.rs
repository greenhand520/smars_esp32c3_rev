//! SMARS 机器人模型层：纯逻辑（无硬件依赖，host 可测）。
//!
//! 所有比例量以协议占空比量纲表示：`-10000..=10000` 对应 -100.00%..100.00%。
//! 各机器人的差异（舵机限位、电池系数等）由参数传入，feature 配置在固件 `config.rs`。

/// WS2812 灯带效果引擎（`ws2812` 特性使用）
pub mod leds;

/// 油门/指令量纲上限（与 STC 协议占空比一致）
pub const FULL_SCALE: i32 = 10_000;

/// 舵机脉宽范围（SG90 通用，500 µs = 物理 0°，2500 µs = 物理 180°）
pub const PULSE_MIN_US: u16 = 500;
pub const PULSE_MAX_US: u16 = 2500;

/// 差速混控：前进/转向 → 左/右油门（-10000..=10000）。
/// 饱和时等比缩放保持转向比。
pub fn mix_diff(forward: i32, turn: i32) -> (i32, i32) {
    let left = forward - turn;
    let right = forward + turn;
    let peak = left.abs().max(right.abs());
    if peak > FULL_SCALE {
        // 等比缩放（先乘后除，保持精度）
        (left * FULL_SCALE / peak, right * FULL_SCALE / peak)
    } else {
        (left, right)
    }
}

/// 油门 → DRV8833 单侧双输入（IN1, IN2 占空比 0..=10000）。
/// 正 = 前进（IN1 PWM），负 = 后退（IN2 PWM），0 = 滑行停止（双 0）。
pub fn throttle_to_pins(throttle: i32) -> (u16, u16) {
    let t = throttle.clamp(-FULL_SCALE, FULL_SCALE);
    if t > 0 {
        (t as u16, 0)
    } else if t < 0 {
        (0, (-t) as u16)
    } else {
        (0, 0)
    }
}

/// 斜坡限速：当前值向目标值步进不超过 max_delta（防急加减速）
pub fn slew(current: i32, target: i32, max_delta: i32) -> i32 {
    if target > current {
        (current + max_delta).min(target)
    } else {
        (current - max_delta).max(target)
    }
}

/// 舵机规格：网页逻辑角度域 → 物理角度域（0..=180°）→ 脉宽
///
/// 例：夹持 0..90° 直接映射物理 0..90°；云台 ±60° 映射物理 30..150°（居中 90°）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServoSpec {
    pub logical_min: i16,
    pub logical_max: i16,
    pub physical_min: u16,
    pub physical_max: u16,
}

impl ServoSpec {
    /// 逻辑角度夹取到有效域
    pub fn clamp(&self, logical: i16) -> i16 {
        logical.clamp(self.logical_min, self.logical_max)
    }

    /// 逻辑角度 → 脉宽（µs）
    pub fn pulse_us(&self, logical: i16) -> u16 {
        let v = self.clamp(logical);
        let span = (self.logical_max - self.logical_min) as i32;
        let phys = if span == 0 {
            self.physical_min as i32
        } else {
            let num = (v - self.logical_min) as i32;
            let phys_span = (self.physical_max - self.physical_min) as i32;
            self.physical_min as i32 + num * phys_span / span
        };
        // 物理角度 0..=180 线性映射脉宽
        (PULSE_MIN_US as i32 + phys * (PULSE_MAX_US - PULSE_MIN_US) as i32 / 180) as u16
    }
}

/// 电池：ADC 采样电压（mV，分压后）→ 电池电压（mV）。
/// `scale_permille` 为分压系数（2:1 分压 = 2000）。
pub fn battery_mv(sample_mv: u32, scale_permille: u32) -> u32 {
    sample_mv * scale_permille / 1000
}

/// 1S 锂电池放电曲线（电压 mV → 电量 %），相邻点间线性插值
const LIPO_CURVE: [(u32, u8); 11] = [
    (4200, 100),
    (4060, 90),
    (3980, 80),
    (3920, 70),
    (3870, 60),
    (3820, 50),
    (3760, 40),
    (3710, 30),
    (3660, 20),
    (3550, 10),
    (3300, 0),
];

/// 电压 → 电量 %（0..=100）
pub fn battery_percent(mv: u32) -> u8 {
    if mv >= LIPO_CURVE[0].0 {
        return 100;
    }
    let last = LIPO_CURVE[LIPO_CURVE.len() - 1];
    if mv <= last.0 {
        return 0;
    }
    for pair in LIPO_CURVE.windows(2) {
        let (hi_mv, hi_pct) = pair[0];
        let (lo_mv, lo_pct) = pair[1];
        if mv <= hi_mv && mv >= lo_mv {
            let span = hi_mv - lo_mv;
            let num = (hi_pct - lo_pct) as u32 * (hi_mv - mv);
            return hi_pct - (num / span) as u8;
        }
    }
    0
}

/// 电池告警级别
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BatteryLevel {
    Ok,
    Low,
    Critical,
}

/// 阈值：低电 3.50V，危急 3.30V（可按机器人调整）
pub const BATTERY_LOW_MV: u32 = 3500;
pub const BATTERY_CRITICAL_MV: u32 = 3300;

/// 欠压切断阈值：低于 3.00V 不响应控制（红色提示 + 固件拒动）
pub const BATTERY_CUTOFF_MV: u32 = 3000;

/// 是否欠压切断（电压有效且低于阈值；0 = 尚未采样，不切断）
pub fn battery_cutoff(mv: u32) -> bool {
    mv != 0 && mv < BATTERY_CUTOFF_MV
}

pub fn battery_level(mv: u32) -> BatteryLevel {
    if mv < BATTERY_CRITICAL_MV {
        BatteryLevel::Critical
    } else if mv < BATTERY_LOW_MV {
        BatteryLevel::Low
    } else {
        BatteryLevel::Ok
    }
}

/// 超声波：回波高电平时间（µs）→ 距离（cm）。
/// 有效量程 2..=400 cm（HC-SR04），超界返回 None（含回波丢失）。
pub fn sonar_cm(echo_us: u32) -> Option<u16> {
    // 声速 343 m/s ≈ 0.0343 cm/µs，往返 ÷2
    let cm = echo_us * 343 / 20000;
    if (2..=400).contains(&cm) {
        Some(cm as u16)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mix_basic() {
        assert_eq!(mix_diff(5000, 0), (5000, 5000));
        assert_eq!(mix_diff(0, 3000), (-3000, 3000));
        assert_eq!(mix_diff(0, -3000), (3000, -3000));
        assert_eq!(mix_diff(0, 0), (0, 0));
    }

    #[test]
    fn mix_saturation_keeps_ratio() {
        // 满油门 + 大转向：超限等比缩放
        let (l, r) = mix_diff(10000, 5000);
        assert!(l.abs() <= FULL_SCALE && r.abs() <= FULL_SCALE);
        assert_eq!((l, r), (3333, 10000));
        let (l, r) = mix_diff(-10000, -10000);
        assert_eq!((l, r), (0, -10000));
    }

    #[test]
    fn throttle_pins() {
        assert_eq!(throttle_to_pins(4000), (4000, 0));
        assert_eq!(throttle_to_pins(-2500), (0, 2500));
        assert_eq!(throttle_to_pins(0), (0, 0));
        // 超界夹取
        assert_eq!(throttle_to_pins(20000), (10000, 0));
        assert_eq!(throttle_to_pins(-20000), (0, 10000));
    }

    #[test]
    fn slew_steps() {
        assert_eq!(slew(0, 100, 30), 30);
        assert_eq!(slew(30, 100, 30), 60);
        assert_eq!(slew(95, 100, 30), 100);
        assert_eq!(slew(0, -100, 40), -40);
        assert_eq!(slew(-95, -100, 40), -100);
        assert_eq!(slew(50, 50, 10), 50);
    }

    #[test]
    fn servo_mapping() {
        // 夹持/上下：0..90 → 物理 0..90
        let s = ServoSpec { logical_min: 0, logical_max: 90, physical_min: 0, physical_max: 90 };
        assert_eq!(s.pulse_us(0), 500);
        assert_eq!(s.pulse_us(90), 1500); // 500 + 90*2000/180
        assert_eq!(s.pulse_us(45), 1000);
        assert_eq!(s.pulse_us(200), s.pulse_us(90)); // 夹取

        // 上下 0..60
        let s = ServoSpec { logical_min: 0, logical_max: 60, physical_min: 0, physical_max: 60 };
        assert_eq!(s.pulse_us(0), 500);
        assert_eq!(s.pulse_us(60), 1166); // 500 + 60*2000/180

        // 云台 ±60 → 物理 30..150（居中 90）
        let s = ServoSpec { logical_min: -60, logical_max: 60, physical_min: 30, physical_max: 150 };
        assert_eq!(s.pulse_us(0), 1500); // 居中
        assert_eq!(s.pulse_us(-60), 833); // 500 + 30*2000/180
        assert_eq!(s.pulse_us(60), 2166); // 500 + 150*2000/180
    }

    #[test]
    fn battery_math() {
        assert_eq!(battery_mv(2000, 2000), 4000); // 2:1 分压
        assert_eq!(battery_mv(1800, 2000), 3600);
        assert_eq!(battery_percent(4200), 100);
        assert_eq!(battery_percent(3300), 0);
        assert_eq!(battery_percent(4300), 100);
        assert_eq!(battery_percent(3200), 0);
        // 3950 位于 3980(80%) 与 3920(70%) 之间
        let p = battery_percent(3950);
        assert!((70..=80).contains(&p), "pct={p}");
        // 单调不减（自 3300 向 4200 扫）
        let mut prev = 0u8;
        for mv in (3300..=4200).step_by(10) {
            let p = battery_percent(mv);
            assert!(p >= prev, "mv={mv} pct={p} prev={prev}");
            prev = p;
        }
    }

    #[test]
    fn battery_levels() {
        assert_eq!(battery_level(3600), BatteryLevel::Ok);
        assert_eq!(battery_level(3500), BatteryLevel::Ok);
        assert_eq!(battery_level(3499), BatteryLevel::Low);
        assert_eq!(battery_level(3300), BatteryLevel::Low);
        assert_eq!(battery_level(3299), BatteryLevel::Critical);
        // 欠压切断：仅 <3.00V 且电压有效（0 = 未采样）
        assert!(!battery_cutoff(0));
        assert!(!battery_cutoff(3000));
        assert!(!battery_cutoff(3200));
        assert!(battery_cutoff(2999));
        assert!(battery_cutoff(2500));
    }

    #[test]
    fn sonar_conversion() {
        assert_eq!(sonar_cm(580), Some(9)); // ≈10cm 取整 9
        assert_eq!(sonar_cm(5800), Some(99));
        assert_eq!(sonar_cm(100), None); // 太近
        assert_eq!(sonar_cm(30000), None); // 太远
    }
}

//! SG90 舵机驱动：LEDC 50 Hz，逻辑角度 → 脉宽（smars_core::ServoSpec）。

use esp_idf_svc::hal::ledc::config::TimerConfig;
use esp_idf_svc::hal::ledc::{LedcDriver, LedcTimerDriver, LowSpeed, Resolution, CHANNEL0, CHANNEL1, TIMER0};
use esp_idf_svc::hal::units::Hertz;
use esp_idf_svc::hal::{gpio::AnyIOPin, sys::EspError};
use log::warn;

/// PWM 周期 20 ms（50 Hz）
const PERIOD_US: u32 = 20_000;

/// 舵机组：最多两路（通道 0 = IO4/PWM9，通道 1 = IO5/PWM10）
pub struct Servos<'d> {
    timer: LedcTimerDriver<'d, LowSpeed>,
    channels: [Option<LedcDriver<'d>>; 2],
}

impl<'d> Servos<'d> {
    /// `pins[i]`：第 i 路舵机引脚（None = 该路不存在）
    /// 占用 LEDC 定时器 0 + 通道 0/1（通道 2 留给蜂鸣器）
    pub fn new(timer0: TIMER0<'d>, ch0: CHANNEL0<'d>, ch1: CHANNEL1<'d>, pins: [Option<AnyIOPin<'d>>; 2]) -> Result<Self, EspError> {
        // 14-bit @ 50Hz：1 tick ≈ 1.22 µs，SG90 脉宽分辨率足够
        let config = TimerConfig::default().frequency(Hertz::from(50)).resolution(Resolution::Bits14);
        let timer = LedcTimerDriver::new(timer0, &config)?;

        let [pin0, pin1] = pins;
        let ch0 = match pin0 {
            Some(pin) => Some(LedcDriver::new(ch0, &timer, pin)?),
            None => None,
        };
        let ch1 = match pin1 {
            Some(pin) => Some(LedcDriver::new(ch1, &timer, pin)?),
            None => None,
        };
        Ok(Self { timer, channels: [ch0, ch1] })
    }

    /// 输出脉宽（µs）到指定通道（0/1）
    pub fn set_pulse_us(&mut self, idx: usize, pulse_us: u16) {
        let Some(ch) = self.channels[idx].as_mut() else { return };
        let max = ch.get_max_duty();
        let duty = ((pulse_us as u32 * max / PERIOD_US) as u32).min(max);
        if let Err(e) = ch.set_duty(duty) {
            warn!("舵机{idx} 设置失败: {e}");
        }
    }
}

//! 状态反馈：IO10 WS2812（RMT 驱动）+ IO2 蜂鸣器（LEDC 音频）。
//!
//! WS2812 时序：T0H=0.3µs/T0L=0.9µs，T1H=0.9µs/T1L=0.3µs，复位 >50µs。
//! 蜂鸣器为无源蜂鸣器，LEDC 50% 占空比方波发声（`tone(freq, ms)`）。

use std::time::Duration;

use esp_idf_svc::hal::delay::Ets;
use esp_idf_svc::hal::gpio::AnyIOPin;
use esp_idf_svc::hal::ledc::config::TimerConfig as LedcTimerConfig;
use esp_idf_svc::hal::ledc::{
    LedcDriver, LedcTimerDriver, LowSpeed, Resolution, CHANNEL2, TIMER1,
};
use esp_idf_svc::hal::rmt::config::TransmitConfig;
use esp_idf_svc::hal::rmt::{
    PinState, Pulse, RmtChannel, TxRmtDriver, VariableLengthSignal,
};
use esp_idf_svc::hal::sys::EspError;
use esp_idf_svc::hal::units::Hertz;
use log::warn;

pub use smars_core::leds::Rgb;

/// 状态灯颜色（GRB 顺序）

pub const COLOR_OK: Rgb = Rgb(0, 32, 0); // 绿
pub const COLOR_LOW: Rgb = Rgb(32, 20, 0); // 黄
pub const COLOR_CRITICAL: Rgb = Rgb(32, 0, 0); // 红
pub const COLOR_ESTOP: Rgb = Rgb(32, 0, 16); // 紫
pub const COLOR_LINK_DOWN: Rgb = Rgb(0, 8, 32); // 蓝

/// WS2812 灯带驱动：整条颜色帧发送（单灯即长度 1 的帧）。
///
/// 位时序：T0H=0.3µs/T0L=0.9µs，T1H=0.9µs/T1L=0.3µs，帧后 >50µs 复位。
pub struct Ws2812<'d> {
    tx: TxRmtDriver<'d>,
    ticks_hz: Hertz,
}

impl<'d> Ws2812<'d> {
    pub fn new(ch: impl RmtChannel + 'd, pin: AnyIOPin<'d>) -> Result<Self, EspError> {
        let config = TransmitConfig::new().clock_divider(1);
        let tx = TxRmtDriver::new(ch, pin, &config)?;
        let ticks_hz = tx.counter_clock()?;
        Ok(Self { tx, ticks_hz })
    }

    /// 发送整条颜色帧（WS2812 线上为 GRB 顺序）
    pub fn show(&mut self, colors: &[Rgb]) -> Result<(), EspError> {
        let mut signal = VariableLengthSignal::with_capacity(colors.len() * 48);
        for c in colors {
            for byte in [c.1, c.0, c.2] {
                for b in 0..8 {
                    let (t_on, t_off) = if byte & (0x80 >> b) != 0 {
                        (900u64, 300u64) // T1H/T1L，ns
                    } else {
                        (300u64, 900u64) // T0H/T0L，ns
                    };
                    let hi = Pulse::new_with_duration(
                        self.ticks_hz,
                        PinState::High,
                        &Duration::from_nanos(t_on),
                    )?;
                    let lo = Pulse::new_with_duration(
                        self.ticks_hz,
                        PinState::Low,
                        &Duration::from_nanos(t_off),
                    )?;
                    signal.push([&hi, &lo])?;
                }
            }
        }
        self.tx.start_blocking(&signal)?;
        // 帧后复位时间（>50µs）
        Ets::delay_us(60);
        Ok(())
    }
}

/// 蜂鸣器（IO2，无源）：LEDC 音频 PWM 驱动，`tone(freq, ms)` 发声。
/// 仅 `buzz` 特性编译（未启用该特性的硬件不焊蜂鸣器）。
#[cfg(feature = "buzz")]
pub struct Buzzer<'d> {
    timer: LedcTimerDriver<'d, LowSpeed>,
    ch: LedcDriver<'d>,
}

#[cfg(feature = "buzz")]
impl<'d> Buzzer<'d> {
    /// 占用 LEDC 定时器 1 + 通道 2（通道 0/1 留给舵机）
    pub fn new(timer1: TIMER1<'d>, ch2: CHANNEL2<'d>, pin: AnyIOPin<'d>) -> Result<Self, EspError> {
        // 初始 2 kHz / 8-bit：任意音高换频后仍满足 LEDC 频率×分辨率上限
        let config = LedcTimerConfig::default().frequency(Hertz::from(2_000)).resolution(Resolution::Bits8);
        let timer = LedcTimerDriver::new(timer1, &config)?;
        let ch = LedcDriver::new(ch2, &timer, pin)?;
        Ok(Self { timer, ch })
    }

    /// 播放 50% 占空比音调，持续 `duration_ms`；`freq_hz = 0` 为静默休止
    pub fn tone(&mut self, freq_hz: u32, duration_ms: u32) {
        if duration_ms == 0 {
            return;
        }
        if freq_hz == 0 {
            self.stop();
            std::thread::sleep(Duration::from_millis(duration_ms as u64));
            return;
        }
        if let Err(e) = self.timer.set_frequency(Hertz::from(freq_hz)) {
            warn!("蜂鸣器换频失败: {e}");
            return;
        }
        let max = self.ch.get_max_duty();
        if let Err(e) = self.ch.set_duty(max / 2) {
            warn!("蜂鸣器发声失败: {e}");
            return;
        }
        std::thread::sleep(Duration::from_millis(duration_ms as u64));
        self.stop();
    }

    /// 停止发声
    pub fn stop(&mut self) {
        let _ = self.ch.set_duty(0);
    }
}

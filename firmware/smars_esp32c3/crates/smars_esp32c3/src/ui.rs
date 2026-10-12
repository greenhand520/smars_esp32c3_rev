//! 状态反馈：IO10 WS2812 单灯（RMT 驱动）+ IO2 蜂鸣器。
//!
//! WS2812 时序：T0H=0.3µs/T0L=0.9µs，T1H=0.9µs/T1L=0.3µs，复位 >50µs。
//! 蜂鸣器为 GPIO 直驱（无源蜂鸣器响度有限，仅作短促提示）。

use std::time::Duration;

use esp_idf_svc::hal::delay::Ets;
use esp_idf_svc::hal::gpio::{AnyIOPin, Output, PinDriver};
use esp_idf_svc::hal::rmt::config::TransmitConfig;
use esp_idf_svc::hal::rmt::{FixedLengthSignal, PinState, Pulse, RmtChannel, TxRmtDriver};
use esp_idf_svc::hal::sys::EspError;
use esp_idf_svc::hal::units::Hertz;
use log::warn;

/// 状态灯颜色（GRB 顺序）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

pub const COLOR_OK: Rgb = Rgb(0, 32, 0); // 绿
pub const COLOR_LOW: Rgb = Rgb(32, 20, 0); // 黄
pub const COLOR_CRITICAL: Rgb = Rgb(32, 0, 0); // 红
pub const COLOR_ESTOP: Rgb = Rgb(32, 0, 16); // 紫
pub const COLOR_LINK_DOWN: Rgb = Rgb(0, 8, 32); // 蓝

/// WS2812 单灯驱动
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

    /// 写一帧 GRB 颜色
    pub fn set_color(&mut self, rgb: Rgb) -> Result<(), EspError> {
        let mut signal = FixedLengthSignal::<24>::new();
        let bits: [u8; 3] = [rgb.1, rgb.0, rgb.2]; // WS2812 为 GRB
        for (i, byte) in bits.iter().enumerate() {
            for b in 0..8 {
                let (t_on, t_off) = if byte & (0x80 >> b) != 0 {
                    (900u64, 300u64) // T1H/T1L，ns
                } else {
                    (300u64, 900u64) // T0H/T0L，ns
                };
                let pair = (
                    Pulse::new_with_duration(
                        self.ticks_hz,
                        PinState::High,
                        &Duration::from_nanos(t_on),
                    )?,
                    Pulse::new_with_duration(
                        self.ticks_hz,
                        PinState::Low,
                        &Duration::from_nanos(t_off),
                    )?,
                );
                signal.set(i * 8 + b, &pair)?;
            }
        }
        self.tx.start_blocking(&signal)?; // 单帧约 30 µs，阻塞发送完成
        // 发送完成后留复位时间（>50µs）
        Ets::delay_us(60);
        Ok(())
    }
}

/// 蜂鸣器（GPIO 直驱）
pub struct Buzzer<'d> {
    pin: PinDriver<'d, Output>,
}

impl<'d> Buzzer<'d> {
    pub fn new(pin: AnyIOPin<'d>) -> Result<Self, EspError> {
        Ok(Self { pin: PinDriver::output(pin)? })
    }

    /// 短促提示音（阻塞）
    pub fn beep(&mut self, times: u8, on_ms: u64, off_ms: u64) {
        for _ in 0..times {
            if let Err(e) = self.pin.set_high() {
                warn!("蜂鸣器失败: {e}");
                return;
            }
            std::thread::sleep(Duration::from_millis(on_ms));
            let _ = self.pin.set_low();
            std::thread::sleep(Duration::from_millis(off_ms));
        }
    }
}

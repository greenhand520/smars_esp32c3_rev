//! HC-SR04 超声波测距：TRIG 脉冲 + ECHO 高电平计时。
//!
//! ECHO 为 5V 电平，硬件须分压/电平转换（design §2）。
//! 阻塞式单次测量（最坏约 25 ms），调用方控制周期。

use std::time::Duration;

use esp_idf_svc::hal::gpio::{Input, Output, PinDriver};
use esp_idf_svc::hal::delay::Ets;
use esp_idf_svc::sys::esp_timer_get_time;

/// 回波等待/宽度上限（对应 ≈ 400 cm 量程）
const TIMEOUT_US: i64 = 25_000;

/// 单次测量：返回距离 cm（smars_core::sonar_cm 语义，超界/超时 None）
pub fn measure_cm(trig: &mut PinDriver<'_, Output>, echo: &mut PinDriver<'_, Input>) -> Option<u16> {
    // 10 µs 触发脉冲
    let _ = trig.set_high();
    Ets::delay_us(10);
    let _ = trig.set_low();

    // 等 ECHO 变高
    let t0 = unsafe { esp_timer_get_time() };
    while echo.is_low() {
        if unsafe { esp_timer_get_time() } - t0 > TIMEOUT_US {
            return None;
        }
    }
    let rise = unsafe { esp_timer_get_time() };
    // 测高电平宽度
    while echo.is_high() {
        if unsafe { esp_timer_get_time() } - rise > TIMEOUT_US {
            return None;
        }
    }
    let fall = unsafe { esp_timer_get_time() };
    smars_core::sonar_cm((fall - rise) as u32)
}

/// 测量节拍建议（连续测量最小间隔）
pub const MEASURE_INTERVAL: Duration = Duration::from_millis(200);

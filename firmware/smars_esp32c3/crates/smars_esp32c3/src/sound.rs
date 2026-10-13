//! 提示音旋律（`buzz` 特性）：开机音 + 汽车喇叭。
//!
//! 旋律参考 servo-robot power manager 的 `startup_sound.rs`（R2-D2 风格扫频），
//! 经 [`crate::ui::Buzzer::tone`] 播放；播放运行在独立线程，不阻塞控制环。

use std::sync::mpsc::Receiver;

use log::warn;

use crate::ui::Buzzer;

/// 提示音请求
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sound {
    /// 开机旋律
    Startup,
    /// 汽车喇叭"滴滴"
    Horn,
}

/// 提示音线程：串行播放队列中的请求
pub fn run(rx: Receiver<Sound>, mut buzzer: Buzzer<'static>) {
    while let Ok(sound) = rx.recv() {
        match sound {
            Sound::Startup => play_startup(&mut buzzer),
            Sound::Horn => play_horn(&mut buzzer),
        }
    }
    warn!("提示音线程退出");
}

/// 汽车喇叭"滴滴"（约 0.35 s）
pub fn play_horn(buzzer: &mut Buzzer) {
    buzzer.tone(2_200, 120);
    buzzer.tone(0, 80);
    buzzer.tone(2_200, 120);
    buzzer.stop();
}

/// R2-D2 风格开机旋律（与参考实现一致）：
/// 快速上升音 → 三声短促高音 → 下行扫频 → 两段上升乐句 → 收尾音。
pub fn play_startup(buzzer: &mut Buzzer) {
    // 快速上升音。
    let mut frequency_hz = 800;
    while frequency_hz <= 3_000 {
        buzzer.tone(frequency_hz, 12);
        frequency_hz += 200;
    }

    // 三声短促高音。
    buzzer.tone(3_500, 30);
    buzzer.tone(0, 15);
    buzzer.tone(4_000, 30);
    buzzer.tone(0, 15);
    buzzer.tone(3_200, 30);
    buzzer.tone(0, 20);

    // 下行扫频。
    let mut frequency_hz = 2_500;
    while frequency_hz >= 600 {
        buzzer.tone(frequency_hz, 10);
        frequency_hz = frequency_hz.saturating_sub(150);
    }

    // 两段欢快的上升乐句。
    buzzer.tone(0, 30);
    let mut frequency_hz = 1_000;
    while frequency_hz <= 2_800 {
        buzzer.tone(frequency_hz, 15);
        frequency_hz += 300;
    }
    buzzer.tone(0, 20);

    let mut frequency_hz = 1_200;
    while frequency_hz <= 3_500 {
        buzzer.tone(frequency_hz, 15);
        frequency_hz += 300;
    }

    // 收尾音。
    buzzer.tone(0, 20);
    buzzer.tone(2_500, 60);
    buzzer.tone(3_000, 80);
    buzzer.stop();
}

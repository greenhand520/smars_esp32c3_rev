//! SMARS 小车主控固件入口（集成于阶段 4：WiFi AP/STA + 网页遥控 + UART 链路）。
//!
//! 当前为可编译骨架：仅初始化日志并空转。

use esp_idf_svc::log::EspLogger;

fn main() {
    esp_idf_svc::sys::link_patches();
    EspLogger::initialize();
    log::info!("smars-esp32c3 boot");
    loop {
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
}

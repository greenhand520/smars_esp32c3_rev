//! WiFi 启动：AP/STA 可切换（design §6）。
//!
//! NVS 命名空间 `wifi` 存 STA 凭据（`ssid`/`pass`）；有凭据则尝试 STA（15 s），
//! 失败回落 AP 热点 `SMARS-<NAME>`；无凭据直接 AP。

use std::time::{Duration, Instant};

use esp_idf_svc::eventloop::EspSystemEventLoop;
use esp_idf_svc::nvs::{EspDefaultNvsPartition, EspNvs, NvsDefault};
use esp_idf_svc::wifi::{
    AccessPointConfiguration, AuthMethod, BlockingWifi, ClientConfiguration,
    Configuration as WifiConfig, EspWifi,
};
use log::{info, warn};

use crate::config::CONFIG;

/// AP 热点口令（8..63 字节，**可改**）
pub const AP_PASSWORD: &str = "smars1234";
/// AP 信道（2.4GHz 1/6/11）
const AP_CHANNEL: u8 = 6;
/// STA 连接超时
const STA_TIMEOUT: Duration = Duration::from_secs(15);

const NVS_NS: &str = "wifi";
const KEY_SSID: &str = "ssid";
const KEY_PASS: &str = "pass";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Ap,
    Sta,
}

/// 网络就绪信息
#[derive(Debug, Clone)]
pub struct NetInfo {
    pub mode: Mode,
    pub ip: String,
}

/// 读取 NVS 中的 STA 凭据
pub fn load_credentials() -> Option<(String, String)> {
    let partition = EspDefaultNvsPartition::take().ok()?;
    let nvs = EspNvs::<NvsDefault>::new(partition, NVS_NS, true).ok()?;
    let mut ssid = [0u8; 33];
    let mut pass = [0u8; 65];
    let s = nvs.get_str(KEY_SSID, &mut ssid).ok()?.map(|s| s.to_string());
    let p = nvs.get_str(KEY_PASS, &mut pass).ok()?.map(|s| s.to_string());
    Some((s?, p.unwrap_or_default()))
}

/// 保存 STA 凭据（下次上电生效）
pub fn save_credentials(ssid: &str, pass: &str) -> Result<(), esp_idf_svc::hal::sys::EspError> {
    let partition = EspDefaultNvsPartition::take()?;
    let nvs = EspNvs::<NvsDefault>::new(partition, NVS_NS, true)?;
    nvs.set_str(KEY_SSID, ssid)?;
    nvs.set_str(KEY_PASS, pass)?;
    Ok(())
}

/// 清除 STA 凭据（回到 AP 模式）
pub fn clear_credentials() -> Result<(), esp_idf_svc::hal::sys::EspError> {
    let partition = EspDefaultNvsPartition::take()?;
    let nvs = EspNvs::<NvsDefault>::new(partition, NVS_NS, true)?;
    nvs.remove(KEY_SSID)?;
    nvs.remove(KEY_PASS)?;
    Ok(())
}

/// 启动 WiFi：优先 STA（有凭据时），失败/无凭据回落 AP
pub fn start(
    wifi: &mut BlockingWifi<EspWifi<'static>>,
    _sysloop: &EspSystemEventLoop,
) -> Result<NetInfo, esp_idf_svc::hal::sys::EspError> {
    if let Some((ssid, pass)) = load_credentials() {
        info!("尝试 STA 连接: {ssid}");
        match try_sta(wifi, &ssid, &pass) {
            Ok(info) => {
                info!("STA 就绪: {}", info.ip);
                return Ok(info);
            }
            Err(e) => warn!("STA 连接失败（{e}），回落 AP"),
        }
    }

    let ssid = format!("SMARS-{}", CONFIG.name.to_uppercase());
    info!("启动 AP 热点: {ssid}");
    let ap = AccessPointConfiguration {
        ssid: ssid.as_str().try_into().unwrap(),
        password: AP_PASSWORD.try_into().unwrap(),
        channel: AP_CHANNEL,
        auth_method: AuthMethod::WPA2Personal,
        ..Default::default()
    };
    wifi.set_configuration(&WifiConfig::AccessPoint(ap))?;
    wifi.start()?;
    wifi.wait_netif_up()?;
    Ok(NetInfo {
        mode: Mode::Ap,
        ip: wifi.wifi().ap_netif().get_ip_info()?.ip.to_string(),
    })
}

fn try_sta(
    wifi: &mut BlockingWifi<EspWifi<'static>>,
    ssid: &str,
    pass: &str,
) -> Result<NetInfo, esp_idf_svc::hal::sys::EspError> {
    let client = ClientConfiguration {
        ssid: ssid.try_into().unwrap(),
        password: pass.try_into().unwrap(),
        auth_method: AuthMethod::WPA2Personal,
        ..Default::default()
    };
    wifi.set_configuration(&WifiConfig::Client(client))?;
    wifi.start()?;
    wifi.connect()?;

    let deadline = Instant::now() + STA_TIMEOUT;
    while Instant::now() < deadline {
        if wifi.is_connected()? {
            wifi.wait_netif_up()?;
            return Ok(NetInfo {
                mode: Mode::Sta,
                ip: wifi.wifi().sta_netif().get_ip_info()?.ip.to_string(),
            });
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    // 超时：断开以便回落 AP
    let _ = wifi.disconnect();
    Err(esp_idf_svc::hal::sys::EspError::from_infallible::<
        { esp_idf_svc::sys::ESP_ERR_TIMEOUT },
    >())
}

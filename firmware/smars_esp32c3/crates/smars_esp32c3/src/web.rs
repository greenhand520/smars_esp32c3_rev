//! HTTP 服务：`GET /`（遥控网页）、`POST /cmd`、`GET /status`、`POST /wifi`。
//!
//! 网页为单页 vanilla HTML/JS（`static/index.html`），按 CONFIG 注入配置
//! （名字/舵机限位/超声波/升降开关），UI 分段动态显隐。

use std::sync::{Arc, Mutex};
use std::time::Duration;

use esp_idf_svc::http::server::{Configuration as HttpConfig, EspHttpServer};
use esp_idf_svc::http::Method;
use log::{info, warn};
use serde_json::{json, Value};

use crate::wifi::NetInfo;
use crate::{config::CONFIG, Shared};

/// 内嵌遥控网页模板
static INDEX_HTML: &str = include_str!("../static/index.html");

/// POST 请求体上限
const MAX_BODY: usize = 512;

/// 注入运行时配置后的完整页面
fn index_page(net: &NetInfo) -> String {
    let servos: Vec<Value> = CONFIG
        .servos
        .iter()
        .enumerate()
        .map(|(idx, s)| {
            json!({
                "idx": idx,
                "ids": s.ids,
                "label": s.label,
                "min": s.spec.logical_min,
                "max": s.spec.logical_max,
            })
        })
        .collect();
    let cfg = json!({
        "name": CONFIG.name,
        "servos": servos,
        "hasSonar": CONFIG.has_sonar,
        "hasLift": CONFIG.has_lift,
        "hasBuzz": cfg!(feature = "buzz"),
        "hasLight": cfg!(feature = "ws2812"),
        "lightCount": crate::config::WS2812_COUNT,
        "mode": if net.mode == crate::wifi::Mode::Ap { "ap" } else { "sta" },
        "ip": net.ip,
    });
    INDEX_HTML.replace("__SMARS_CONFIG__", &cfg.to_string())
}

/// 启动 HTTP 服务（阻塞式 handler，运行在 httpd 任务上）
pub fn start(
    shared: Arc<Mutex<Shared>>,
    net: &NetInfo,
) -> anyhow::Result<EspHttpServer<'static>> {
    let page = Arc::new(index_page(net));

    let mut server = EspHttpServer::new(&HttpConfig {
        stack_size: 8192, // 浏览器请求头较长
        ..Default::default()
    })?;

    // 遥控页面
    {
        let page = page.clone();
        server.fn_handler("/", Method::Get, move |request| {
            request
                .into_ok_response()?
                .write(page.as_bytes())?;
            Ok(()) as anyhow::Result<()>
        })?;
    }

    // 遥测轮询
    {
        let shared = shared.clone();
        server.fn_handler("/status", Method::Get, move |request| {
            let s = shared.lock().unwrap_or_else(|e| e.into_inner());
            let servos: Vec<Value> = CONFIG
                .servos
                .iter()
                .enumerate()
                .map(|(idx, sv)| {
                    json!({"idx": idx, "ids": sv.ids, "label": sv.label, "angle": s.servo_angle[idx]})
                })
                .collect();
            let body = json!({
                "name": CONFIG.name,
                "estop": s.estop,
                "stcOk": s.stc_ok,
                "batteryMv": s.battery_mv,
                "batteryPct": s.battery_pct,
                "batteryLevel": battery_level_str(&s),
                "sonarCm": s.sonar_cm,
                "forward": s.forward,
                "turn": s.turn,
                "lift": s.lift,
                "servos": servos,
                "light": {
                    "on": s.light_on,
                    "effect": smars_core::leds::Effect::ALL
                        .get(s.light_effect as usize)
                        .map(|e| e.id())
                        .unwrap_or("breath"),
                    "brightness": s.light_brightness,
                },
            });
            let mut resp = request.into_response(200, Some("OK"), &[("Content-Type", "application/json")])?;
            resp.write(body.to_string().as_bytes())?;
            Ok(()) as anyhow::Result<()>
        })?;
    }

    // 命令下发（驾驶/升降/舵机/急停/恢复）
    {
        let shared = shared.clone();
        server.fn_handler("/cmd", Method::Post, move |mut request| {
            let mut buf = [0u8; MAX_BODY];
            let n = request.read(&mut buf)?;
            let body = String::from_utf8_lossy(&buf[..n]).to_string();
            let resp = handle_cmd(&shared, &body);
            request
                .into_response(resp.0, Some(resp.1), &[("Content-Type", "application/json")])?
                .write(resp.2.as_bytes())?;
            Ok(()) as anyhow::Result<()>
        })?;
    }

    // WiFi 设置（保存后重启生效）
    server.fn_handler("/wifi", Method::Post, move |mut request| {
        let mut buf = [0u8; MAX_BODY];
        let n = request.read(&mut buf)?;
        let body = String::from_utf8_lossy(&buf[..n]).to_string();
        let (code, msg, out) = handle_wifi(&body);
        request
            .into_response(code, Some(msg), &[("Content-Type", "application/json")])?
            .write(out.as_bytes())?;
        if code == 200 {
            std::thread::spawn(|| {
                std::thread::sleep(Duration::from_millis(500));
                info!("WiFi 配置已保存，重启生效");
                unsafe { esp_idf_svc::sys::esp_restart() };
            });
        }
        Ok(()) as anyhow::Result<()>
    })?;

    info!("HTTP 服务就绪");
    Ok(server)
}

fn battery_level_str(s: &Shared) -> &'static str {
    match smars_core::battery_level(s.battery_mv) {
        smars_core::BatteryLevel::Ok => "ok",
        smars_core::BatteryLevel::Low => "low",
        smars_core::BatteryLevel::Critical => "critical",
    }
}

/// 处理 /cmd：JSON 对象，可含多个动作键
/// 返回 (状态码, 原因, JSON 响应体)
fn handle_cmd(shared: &Arc<Mutex<Shared>>, body: &str) -> (u16, &'static str, String) {
    let Ok(v) = serde_json::from_str::<Value>(body) else {
        return (400, "Bad Request", r#"{"ok":false}"#.into());
    };
    let mut s = shared.lock().unwrap_or_else(|e| e.into_inner());

    if v.get("estop").and_then(Value::as_bool) == Some(true) {
        s.estop_pending = Some(true);
    }
    if v.get("resume").and_then(Value::as_bool) == Some(true) {
        s.estop_pending = Some(false);
    }
    // 喇叭（buzz 特性；蜂鸣器线程播放）
    if v.get("horn").and_then(Value::as_i64) == Some(1) {
        s.horn_requested = true;
    }
    // 灯带（ws2812 特性）：开关 / 效果 / 亮度
    if let Some(on) = v.get("lightOn").and_then(Value::as_bool) {
        s.light_on = on;
    }
    if let Some(effect) = v.get("effect").and_then(Value::as_str) {
        if let Some(idx) = smars_core::leds::Effect::ALL.iter().position(|e| e.id() == effect) {
            s.light_effect = idx as u8;
        }
    }
    if let Some(b) = v.get("brightness").and_then(Value::as_u64) {
        s.light_brightness = (b as u8).min(100);
    }

    let mut motion = false;
    if let Some(f) = v.get("forward").and_then(Value::as_i64) {
        if !s.estop {
            s.forward = (f as i32).clamp(-10_000, 10_000);
            motion = true;
        }
    }
    if let Some(t) = v.get("turn").and_then(Value::as_i64) {
        if !s.estop {
            s.turn = (t as i32).clamp(-10_000, 10_000);
            motion = true;
        }
    }
    if motion {
        s.last_drive = std::time::Instant::now();
    }

    if let Some(l) = v.get("lift").and_then(Value::as_i64) {
        if !s.estop && CONFIG.has_lift {
            s.lift = l.clamp(-1, 1) as i8;
            s.last_lift = std::time::Instant::now();
        }
    }

    if let Some(sv) = v.get("servo").and_then(Value::as_u64) {
        if let Some(a) = v.get("angle").and_then(Value::as_i64) {
            let idx = sv as usize;
            if let Some(def) = CONFIG.servos.get(idx) {
                let angle = (a as i16).clamp(def.spec.logical_min, def.spec.logical_max);
                s.servo_angle[idx] = angle;
                s.servo_dirty = true;
            }
        }
    }

    (200, "OK", r#"{"ok":true}"#.into())
}

/// 处理 /wifi：{"ssid": "...", "password": "..."} 或 {"clear": true}
fn handle_wifi(body: &str) -> (u16, &'static str, String) {
    let Ok(v) = serde_json::from_str::<Value>(body) else {
        return (400, "Bad Request", r#"{"ok":false}"#.into());
    };
    if v.get("clear").and_then(Value::as_bool) == Some(true) {
        return match crate::wifi::clear_credentials() {
            Ok(()) => (200, "OK", r#"{"ok":true}"#.into()),
            Err(e) => {
                warn!("清除凭据失败: {e}");
                (500, "Internal Server Error", r#"{"ok":false}"#.into())
            }
        };
    }
    let Some(ssid) = v.get("ssid").and_then(Value::as_str) else {
        return (400, "Bad Request", r#"{"ok":false}"#.into());
    };
    let pass = v.get("password").and_then(Value::as_str).unwrap_or("");
    if ssid.is_empty() || ssid.len() > 32 || pass.len() > 64 {
        return (400, "Bad Request", r#"{"ok":false}"#.into());
    }
    match crate::wifi::save_credentials(ssid, pass) {
        Ok(()) => (200, "OK", r#"{"ok":true}"#.into()),
        Err(e) => {
            warn!("保存凭据失败: {e}");
            (500, "Internal Server Error", r#"{"ok":false}"#.into())
        }
    }
}

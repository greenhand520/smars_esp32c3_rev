//! SMARS 小车主控固件入口（docs/design.md）。
//!
//! 装配：WiFi（AP/STA 可切换）+ 网页遥控 HTTP + UART0 ↔ STC8H1K08（协议 v2）+
//! 舵机/超声波/电池/状态灯/蜂鸣器；控制线程 50 ms 节拍，含失联停车看门狗。

mod battery;
mod config;
mod drive;
mod servo;
mod sonar;
#[cfg(feature = "buzz")]
mod sound;
mod stc_link;
mod ui;
mod web;
mod wifi;

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use esp_idf_svc::eventloop::EspSystemEventLoop;
use esp_idf_svc::hal::gpio::{AnyIOPin, PinDriver, Pull};
use esp_idf_svc::hal::peripherals::Peripherals;
use esp_idf_svc::hal::uart::{config::Config as UartConfig, UartDriver};
use esp_idf_svc::hal::uart;
use esp_idf_svc::log::EspLogger;
use esp_idf_svc::wifi::{BlockingWifi, EspWifi};
use log::{info, warn};

use config::CONFIG;
use stc_link::StcLink;

/// 控制周期
const CONTROL_TICK: Duration = Duration::from_millis(50);
/// 驾驶失联停车时限（网页按住期间 200 ms 心跳，1 s 无命令即停）
const DRIVE_TIMEOUT: Duration = Duration::from_secs(1);
/// 升降单次按压上限（design §7）
const LIFT_TIMEOUT: Duration = Duration::from_secs(3);
/// 斜坡步进（每控制周期油门变化上限，防急加减速）
const SLEW_STEP: i32 = 1_500;

/// 网页线程 ↔ 控制线程共享状态
pub struct Shared {
    pub forward: i32,
    pub turn: i32,
    pub lift: i8,
    /// 舵机目标（逻辑角，下标 = id-1）
    pub servo_angle: [i16; 2],
    pub servo_dirty: bool,
    pub estop: bool,
    /// 网页请求的急停/恢复（控制线程执行 STC 命令后清零）
    pub estop_pending: Option<bool>,
    /// 喇叭请求（buzz 特性，控制环转发给提示音线程）
    pub horn_requested: bool,
    /// 灯带（ws2812 特性）：开关 / 效果（Effect::ALL 下标）/ 亮度 0..=100
    pub light_on: bool,
    pub light_effect: u8,
    pub light_brightness: u8,
    pub battery_mv: u32,
    pub battery_pct: u8,
    pub sonar_cm: Option<u16>,
    pub stc_ok: bool,
    pub last_drive: Instant,
    pub last_lift: Instant,
}

impl Default for Shared {
    fn default() -> Self {
        Self {
            forward: 0,
            turn: 0,
            lift: 0,
            servo_angle: [0; 2],
            servo_dirty: false,
            estop: false,
            estop_pending: None,
            horn_requested: false,
            light_on: true,
            light_effect: 1, // 呼吸
            light_brightness: 60,
            battery_mv: 0,
            battery_pct: 0,
            sonar_cm: None,
            stc_ok: false,
            last_drive: Instant::now(),
            last_lift: Instant::now(),
        }
    }
}

fn main() {
    esp_idf_svc::sys::link_patches();
    EspLogger::initialize_default();
    info!("SMARS-{} 启动（协议 v{}）", CONFIG.name, smars_protocol::PROTO_VERSION);

    let peripherals = Peripherals::take().unwrap();
    let sysloop = EspSystemEventLoop::take().unwrap();

    // ---- WiFi + HTTP ----
    let mut wifi = BlockingWifi::wrap(
        EspWifi::new(peripherals.modem, sysloop.clone(), None).unwrap(),
        sysloop.clone(),
    )
    .unwrap();
    let net = wifi::start(&mut wifi, &sysloop).expect("WiFi 启动失败");
    info!("网络就绪: {:?} {}", net.mode, net.ip);

    let shared = Arc::new(Mutex::new(Shared::default()));
    let _server = web::start(shared.clone(), &net).expect("HTTP 服务启动失败");

    // ---- 硬件 ----
    // 舵机：按配置取 id 1=IO4 / id 2=IO5
    let has_servo1 = CONFIG.servos.iter().any(|s| s.id == 1);
    let has_servo2 = CONFIG.servos.iter().any(|s| s.id == 2);
    let servo_pins: [Option<AnyIOPin>; 2] = [
        if has_servo1 { Some(peripherals.pins.gpio4.into()) } else { None },
        if has_servo2 { Some(peripherals.pins.gpio5.into()) } else { None },
    ];
    let mut servos = servo::Servos::new(
        peripherals.ledc.timer0,
        peripherals.ledc.channel0,
        peripherals.ledc.channel1,
        servo_pins,
    )
    .expect("舵机初始化失败");
    // 上电先输出初始角（0°，云台为中位），避免舵机上电漂移
    for def in CONFIG.servos {
        servos.set_pulse_us(def.id as usize - 1, def.spec.pulse_us(0));
    }

    // 超声波（无该外设时引脚仍占位，测量任务跳过）
    let mut trig = PinDriver::output(peripherals.pins.gpio6).expect("TRIG 初始化失败");
    let mut echo =
        PinDriver::input(peripherals.pins.gpio7, Pull::Floating).expect("ECHO 初始化失败");

    // 电池检测
    let mut bat = battery::Battery::new(peripherals.adc1, peripherals.pins.gpio3).expect("ADC 初始化失败");

    // 状态灯/灯带
    let mut led = ui::Ws2812::new(peripherals.rmt.channel0, peripherals.pins.gpio10.into())
        .expect("WS2812 初始化失败");
    // 蜂鸣器（buzz 特性；未启用的硬件不焊蜂鸣器，固件不初始化、不发声）
    #[cfg(feature = "buzz")]
    let sound_tx = {
        let buzzer = ui::Buzzer::new(
            peripherals.ledc.timer1,
            peripherals.ledc.channel2,
            peripherals.pins.gpio2.into(),
        )
        .expect("蜂鸣器初始化失败");
        let (tx, rx) = std::sync::mpsc::channel::<sound::Sound>();
        std::thread::spawn(move || sound::run(rx, buzzer));
        let _ = tx.send(sound::Sound::Startup);
        tx
    };

    // ---- UART0 ↔ STC8H1K08（115200 8N1，TX=IO21，RX=IO20）----
    let uart_config = UartConfig::new()
        .baudrate(stc_link::BAUDRATE.into())
        .data_bits(uart::config::DataBits::DataBits8)
        .parity_none()
        .stop_bits(uart::config::StopBits::STOP1)
        .flow_control(uart::config::FlowControl::None);
    let uart = UartDriver::new(
        peripherals.uart0,
        peripherals.pins.gpio21,
        peripherals.pins.gpio20,
        Option::<AnyIOPin>::None,
        Option::<AnyIOPin>::None,
        &uart_config,
    )
    .expect("UART0 初始化失败");
    let mut stc = StcLink::new(uart);

    info!("初始化完成，进入控制循环");
    #[cfg(not(feature = "ws2812"))]
    let _ = led.show(&[ui::COLOR_OK]);

    // ---- 控制循环 ----
    let mut fwd_slew = 0i32;
    let mut turn_slew = 0i32;
    let mut last_send = Instant::now() - Duration::from_secs(1);
    let mut last_sent: Option<(i32, i32, i32)> = None;
    let mut last_sonar = Instant::now();
    let mut last_bat = Instant::now();
    let mut sent_estop = false;
    #[cfg(feature = "ws2812")]
    let light_start = Instant::now();

    loop {
        // 取目标（网页线程写入）
        let (mut fwd, mut turn, mut lift, estop_req, horn, servo_dirty, servo_angles) = {
            let mut s = shared.lock().unwrap_or_else(|e| e.into_inner());
            // 看门狗：失联停车
            if s.last_drive.elapsed() > DRIVE_TIMEOUT {
                s.forward = 0;
                s.turn = 0;
            }
            if s.last_lift.elapsed() > LIFT_TIMEOUT {
                s.lift = 0;
            }
            let horn = std::mem::take(&mut s.horn_requested);
            (
                s.forward,
                s.turn,
                s.lift,
                s.estop_pending.take(),
                horn,
                std::mem::take(&mut s.servo_dirty),
                s.servo_angle,
            )
        };

        // 喇叭（buzz 特性）
        #[cfg(feature = "buzz")]
        if horn {
            let _ = sound_tx.send(sound::Sound::Horn);
        }
        #[cfg(not(feature = "buzz"))]
        let _ = horn;

        // 急停/恢复（STC 侧锁存语义，design §4.4）
        if let Some(enter) = estop_req {
            let r = stc.request(if enter { smars_protocol::Command::Estop } else { smars_protocol::Command::Resume });
            match r {
                Ok(_) => {
                    sent_estop = enter;
                    let mut s = shared.lock().unwrap_or_else(|e| e.into_inner());
                    s.estop = enter;
                    if enter {
                        s.forward = 0;
                        s.turn = 0;
                        s.lift = 0;
                    }
                    fwd = 0;
                    turn = 0;
                    lift = 0;
                    info!("{}", if enter { "急停" } else { "恢复" });
                    #[cfg(not(feature = "ws2812"))]
                    let _ = led.show(&[if enter { ui::COLOR_ESTOP } else { ui::COLOR_OK }]);
                }
                Err(e) => warn!("急停命令失败: {e}"),
            }
        }
        if sent_estop {
            fwd = 0;
            turn = 0;
            lift = 0;
        }

        // 斜坡
        fwd_slew = smars_core::slew(fwd_slew, fwd, SLEW_STEP);
        turn_slew = smars_core::slew(turn_slew, turn, SLEW_STEP);

        // 舵机（变化时下发）
        if servo_dirty {
            for def in CONFIG.servos {
                let angle = servo_angles[def.id as usize - 1];
                servos.set_pulse_us(def.id as usize - 1, def.spec.pulse_us(angle));
            }
        }

        // STC 输出（状态变化或 500 ms 心跳）
        let want = (fwd_slew, turn_slew, lift as i32);
        if last_sent != Some(want) || last_send.elapsed() > Duration::from_millis(500) {
            let lift_duty = lift as i32 * smars_core::FULL_SCALE;
            let cmd = drive::set_all_command(fwd_slew, turn_slew, lift_duty);
            match stc.request(cmd) {
                Ok(_) => {
                    let mut s = shared.lock().unwrap_or_else(|e| e.into_inner());
                    if !s.stc_ok {
                        info!("STC 链路恢复");
                    }
                    s.stc_ok = true;
                    last_sent = Some(want);
                    last_send = Instant::now();
                }
                Err(e) => {
                    let mut s = shared.lock().unwrap_or_else(|e| e.into_inner());
                    if s.stc_ok {
                        warn!("STC 链路异常: {e}");
                        #[cfg(not(feature = "ws2812"))]
                        let _ = led.show(&[ui::COLOR_LINK_DOWN]);
                    }
                    s.stc_ok = false;
                }
            }
        }

        // 超声波（每 200 ms）
        if CONFIG.has_sonar && last_sonar.elapsed() >= sonar::MEASURE_INTERVAL {
            let cm = sonar::measure_cm(&mut trig, &mut echo);
            shared.lock().unwrap_or_else(|e| e.into_inner()).sonar_cm = cm;
            last_sonar = Instant::now();
        }

        // 电池（每 1 s）+ 状态灯
        if last_bat.elapsed() >= Duration::from_secs(1) {
            if let Some(mv) = bat.sample_mv(CONFIG.battery_scale_permille) {
                let pct = smars_core::battery_percent(mv);
                let level = smars_core::battery_level(mv);
                let mut s = shared.lock().unwrap_or_else(|e| e.into_inner());
                s.battery_mv = mv;
                s.battery_pct = pct;
                drop(s);
                if !sent_estop {
                    #[cfg(not(feature = "ws2812"))]
                    let _ = led.show(&[match level {
                        smars_core::BatteryLevel::Ok => ui::COLOR_OK,
                        smars_core::BatteryLevel::Low => ui::COLOR_LOW,
                        smars_core::BatteryLevel::Critical => ui::COLOR_CRITICAL,
                    }]);
                    #[cfg(feature = "ws2812")]
                    let _ = level; // 灯带由效果引擎控制，状态色不抢灯
                }
            }
            last_bat = Instant::now();
        }

        // 灯带效果（ws2812 特性）：每周期渲染并发送
        #[cfg(feature = "ws2812")]
        {
            let (on, effect, brightness) = {
                let s = shared.lock().unwrap_or_else(|e| e.into_inner());
                (s.light_on, s.light_effect, s.light_brightness)
            };
            let mut frame = [smars_core::leds::Rgb(0, 0, 0); config::WS2812_COUNT];
            if on {
                let eff = *smars_core::leds::Effect::ALL
                    .get(effect as usize)
                    .unwrap_or(&smars_core::leds::Effect::Breath);
                smars_core::leds::render(
                    eff,
                    light_start.elapsed().as_millis() as u32,
                    brightness,
                    &mut frame,
                );
            }
            if let Err(e) = led.show(&frame) {
                warn!("灯带发送失败: {e}");
            }
        }

        std::thread::sleep(CONTROL_TICK);
    }
}

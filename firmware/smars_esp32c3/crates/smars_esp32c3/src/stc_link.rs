//! STC8H1K08 链路：UART0 请求/应答（协议 v2）+ 超时重试 + 链路状态。
//!
//! 串口参数与固件侧一致：115200 8N1；请求后等应答，超时 20 ms ×3 重试。
//! 收发字节流经 `smars_protocol::Decoder` 解析。

use std::sync::Mutex;
use std::time::{Duration, Instant};

use esp_idf_svc::hal::delay::TickType;
use esp_idf_svc::hal::uart::UartDriver;
use log::warn;
use smars_protocol::{Command, Decoder, Event, Response, Status};

/// 波特率（协议 §4.5）
pub const BAUDRATE: u32 = 115_200;
/// 单次应答等待超时
const RESP_TIMEOUT: Duration = Duration::from_millis(20);
/// 最大重试次数（1 次 + 重试 3 次）
const MAX_ATTEMPTS: u32 = 3;
/// 每次尝试间读取缓冲上限
const RX_CHUNK: usize = 256;

/// 链路错误
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkError {
    /// 应答超时（重试用尽）
    Timeout,
    /// 应答状态非 OK（如急停锁存中、参数错）
    Nack(Status),
    /// 串口 I/O 错误
    Io,
}

impl core::fmt::Display for LinkError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            LinkError::Timeout => write!(f, "应答超时"),
            LinkError::Nack(s) => write!(f, "NACK({:?})", s),
            LinkError::Io => write!(f, "串口错误"),
        }
    }
}

/// STC 链路（UART0 + 协议编解码）
pub struct StcLink<'d> {
    uart: UartDriver<'d>,
    decoder: Decoder,
    /// 最近一次成功应答时刻（链路存活判据）
    last_ok: Option<Instant>,
}

impl<'d> StcLink<'d> {
    pub fn new(uart: UartDriver<'d>) -> Self {
        Self { uart, decoder: Decoder::new(), last_ok: None }
    }

    /// 发送命令并等对应应答（超时重试）。应答状态非 OK 返回 `Nack`。
    pub fn request(&mut self, cmd: Command) -> Result<Response, LinkError> {
        let frame = cmd.encode_request();
        let want = cmd.cmd_byte() | smars_protocol::RESP_FLAG;
        let mut last_err = LinkError::Timeout;

        for _ in 0..MAX_ATTEMPTS {
            self.uart
                .write(&frame)
                .map_err(|_| LinkError::Io)?;

            match self.wait_response(want) {
                Ok(resp) => {
                    self.last_ok = Some(Instant::now());
                    return if resp.status == Status::Ok {
                        Ok(resp)
                    } else {
                        Err(LinkError::Nack(resp.status))
                    };
                }
                Err(e) => last_err = e,
            }
            warn!("STC 应答失败重试: {last_err}");
        }
        Err(last_err)
    }

    /// 等待指定命令的应答帧（含杂音丢弃/超时）
    fn wait_response(&mut self, want_cmd: u8) -> Result<Response, LinkError> {
        let deadline = Instant::now() + RESP_TIMEOUT;
        let mut buf = [0u8; RX_CHUNK];

        loop {
            let remain = deadline.saturating_duration_since(Instant::now());
            if remain.is_zero() {
                return Err(LinkError::Timeout);
            }
            let n = self
                .uart
                .read(&mut buf, TickType::from(remain).ticks())
                .map_err(|_| LinkError::Io)?;
            let now_ms = elapsed_ms();
            for &b in &buf[..n] {
                if let Some(ev) = self.decoder.feed(b, now_ms) {
                    match ev {
                        Event::Frame(resp) if resp.cmd == want_cmd => return Ok(resp),
                        // 其他帧（旧应答/杂音）跳过
                        Event::Frame(_) => {}
                        Event::CrcError { .. } | Event::LenError { .. } => {}
                    }
                }
            }
        }
    }

    /// 链路是否存活（最近 N 秒内有成功应答）
    pub fn link_alive(&self, within: Duration) -> bool {
        self.last_ok.map(|t| t.elapsed() < within).unwrap_or(false)
    }
}

/// 环境时钟毫秒（供协议超时用）
pub fn elapsed_ms() -> u32 {
    // 单调时钟足够；协议用 wrapping 差值判断
    static START: Mutex<Option<Instant>> = Mutex::new(None);
    let mut start = START.lock().unwrap_or_else(|e| e.into_inner());
    let start = start.get_or_insert_with(Instant::now);
    start.elapsed().as_millis() as u32
}

//! STC8H1K08 八路 PWM 控制器串口协议 v2（主机端实现）。
//!
//! 线上格式（与固件 `firmware/stc8h1k08_pwm/docs/design.md` §4 一致）：
//!
//! ```text
//! 请求帧：  AA 55 | cmd | len | payload[len] | crc8
//! 应答帧：  AA 55 | cmd|0x80 | len | status | data... | crc8
//! crc8 覆盖 cmd..payload（第 2..3+len 字节），多项式 0x07，初值 0x00。
//! ```
//!
//! 多字节整数一律大端序；占空比 0..=10000 对应 0.00%..100.00%。

/// 协议版本（`Command::Version` 应答首字节）
pub const PROTO_VERSION: u8 = 2;

const HDR1: u8 = 0xAA;
const HDR2: u8 = 0x55;
/// 应答帧命令字节标志（请求命令 `| RESP_FLAG`）
pub const RESP_FLAG: u8 = 0x80;
/// 数据区最大长度（超出即非法长度）
pub const MAX_PAYLOAD: usize = 32;
/// 帧内字节间最大间隔（毫秒），超出则中止未完成帧
pub const TIMEOUT_MS: u32 = 5;

/// 查询应答数据区长度（`ActualState::from_data`）
pub const QUERY_DATA_LEN: usize = 25;

/// CRC-8/ATM：多项式 0x07，初值 0x00，不反转，无异或输出
pub fn crc8(data: &[u8]) -> u8 {
    let mut crc = 0u8;
    for &b in data {
        crc ^= b;
        for _ in 0..8 {
            crc = if crc & 0x80 != 0 { (crc << 1) ^ 0x07 } else { crc << 1 };
        }
    }
    crc
}

/// 应答状态码（应答帧数据区首字节）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Ok,
    Crc,
    Len,
    Param,
    Estop,
    Unknown(u8),
}

impl Status {
    pub fn from_u8(v: u8) -> Self {
        match v {
            0x00 => Status::Ok,
            0x01 => Status::Crc,
            0x02 => Status::Len,
            0x03 => Status::Param,
            0x04 => Status::Estop,
            other => Status::Unknown(other),
        }
    }

    pub fn as_u8(self) -> u8 {
        match self {
            Status::Ok => 0x00,
            Status::Crc => 0x01,
            Status::Len => 0x02,
            Status::Param => 0x03,
            Status::Estop => 0x04,
            Status::Unknown(v) => v,
        }
    }
}

/// 请求命令
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    /// `0x01` 全量设置：A 组频率、B 组频率、8 路占空比
    SetAll { freq_a: u16, freq_b: u16, duties: [u16; 8] },
    /// `0x02` 单通道占空比（通道 0..=7）
    SetDuty { channel: u8, duty: u16 },
    /// `0x03` 全量占空比（频率不变）
    SetDuties([u16; 8]),
    /// `0x04` 急停（锁存，8 路恒低）
    Estop,
    /// `0x05` 恢复（解除急停锁存，还原配置）
    Resume,
    /// `0x06` 状态查询（回读实际生效值）
    Query,
    /// `0x07` 版本查询
    Version,
}

impl Command {
    /// 线上命令字节
    pub fn cmd_byte(self) -> u8 {
        match self {
            Command::SetAll { .. } => 0x01,
            Command::SetDuty { .. } => 0x02,
            Command::SetDuties(_) => 0x03,
            Command::Estop => 0x04,
            Command::Resume => 0x05,
            Command::Query => 0x06,
            Command::Version => 0x07,
        }
    }

    /// 编码为完整请求帧
    pub fn encode_request(self) -> Vec<u8> {
        let mut payload = Vec::with_capacity(20);
        match self {
            Command::SetAll { freq_a, freq_b, duties } => {
                payload.extend_from_slice(&freq_a.to_be_bytes());
                payload.extend_from_slice(&freq_b.to_be_bytes());
                for d in duties {
                    payload.extend_from_slice(&d.to_be_bytes());
                }
            }
            Command::SetDuty { channel, duty } => {
                payload.push(channel);
                payload.extend_from_slice(&duty.to_be_bytes());
            }
            Command::SetDuties(duties) => {
                for d in duties {
                    payload.extend_from_slice(&d.to_be_bytes());
                }
            }
            Command::Estop | Command::Resume | Command::Query | Command::Version => {}
        }
        encode_frame(self.cmd_byte(), &payload)
    }
}

/// 编码一帧：`AA 55 | cmd | len | payload | crc8`
pub fn encode_frame(cmd: u8, payload: &[u8]) -> Vec<u8> {
    debug_assert!(payload.len() <= MAX_PAYLOAD);
    let mut frame = Vec::with_capacity(payload.len() + 5);
    frame.push(HDR1);
    frame.push(HDR2);
    frame.push(cmd);
    frame.push(payload.len() as u8);
    frame.extend_from_slice(payload);
    let crc = crc8(&frame[2..]);
    frame.push(crc);
    frame
}

/// 收到的应答帧
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    /// 命令字节（应答为请求命令 `| RESP_FLAG`）
    pub cmd: u8,
    pub status: Status,
    /// 状态之后的数据区
    pub data: Vec<u8>,
}

/// 解析事件（与固件侧状态机语义一致）
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// 帧完整且 CRC 正确
    Frame(Response),
    /// CRC 错，帧被丢弃
    CrcError { cmd: u8 },
    /// 长度字段非法（> MAX_PAYLOAD），帧被丢弃
    LenError { cmd: u8 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseError {
    BadLength,
}

/// 流式帧解析器（喂字节 + 毫秒时间戳，支持帧间超时重同步）
#[derive(Debug, Clone)]
pub struct Decoder {
    state: State,
    cmd: u8,
    len: u8,
    idx: u8,
    crc: u8,
    last_ms: u32,
    payload: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Hdr1,
    Hdr2,
    Cmd,
    Len,
    Data,
    Crc,
}

impl Default for Decoder {
    fn default() -> Self {
        Self::new()
    }
}

impl Decoder {
    pub fn new() -> Self {
        Self {
            state: State::Hdr1,
            cmd: 0,
            len: 0,
            idx: 0,
            crc: 0,
            last_ms: 0,
            payload: Vec::with_capacity(MAX_PAYLOAD),
        }
    }

    /// 喂入一个字节；`now_ms` 为单调毫秒计数。
    /// 字节间隔 `> TIMEOUT_MS` 会中止未完成的帧。
    pub fn feed(&mut self, byte: u8, now_ms: u32) -> Option<Event> {
        // 字节间隔超时：丢弃未完成的帧，随后在帧头搜索态重新同步
        if self.state != State::Hdr1 && now_ms.wrapping_sub(self.last_ms) > TIMEOUT_MS {
            self.state = State::Hdr1;
        }
        self.last_ms = now_ms;

        match self.state {
            State::Hdr1 => {
                if byte == HDR1 {
                    self.state = State::Hdr2;
                }
            }
            State::Hdr2 => match byte {
                HDR2 => self.state = State::Cmd,
                HDR1 => self.state = State::Hdr2, // 容忍 AA AA 55
                _ => self.state = State::Hdr1,
            },
            State::Cmd => {
                self.cmd = byte;
                self.crc = crc8(&[byte]);
                self.state = State::Len;
            }
            State::Len => {
                if byte as usize > MAX_PAYLOAD {
                    self.state = State::Hdr1;
                    return Some(Event::LenError { cmd: self.cmd });
                }
                self.len = byte;
                self.idx = 0;
                self.crc = crc_step(self.crc, byte);
                self.state = if byte == 0 { State::Crc } else { State::Data };
            }
            State::Data => {
                self.payload.push(byte);
                self.idx += 1;
                self.crc = crc_step(self.crc, byte);
                if self.idx >= self.len {
                    self.state = State::Crc;
                }
            }
            State::Crc => {
                self.state = State::Hdr1;
                if byte == self.crc {
                    let mut data = std::mem::replace(&mut self.payload, Vec::with_capacity(MAX_PAYLOAD));
                    // 应答帧：首字节为状态码，其余为数据区
                    let status_v = if self.cmd & RESP_FLAG != 0 && !data.is_empty() {
                        data.remove(0)
                    } else {
                        0x00
                    };
                    return Some(Event::Frame(Response {
                        cmd: self.cmd,
                        status: Status::from_u8(status_v),
                        data,
                    }));
                }
                self.payload.clear();
                return Some(Event::CrcError { cmd: self.cmd });
            }
        }
        None
    }
}

fn crc_step(crc: u8, byte: u8) -> u8 {
    let mut crc = crc ^ byte;
    for _ in 0..8 {
        crc = if crc & 0x80 != 0 { (crc << 1) ^ 0x07 } else { crc << 1 };
    }
    crc
}

/// `0x06` 查询回读：实际生效值（量化后占空比、每组分辨率、实际频率）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActualState {
    pub flags: u8,
    /// A 组实际频率 Hz（0 = 停）
    pub freq_a: u16,
    /// B 组实际频率 Hz（0 = 停）
    pub freq_b: u16,
    /// A 组每周期计数步 ARR+1（0 = 未配置）
    pub n_a: u16,
    /// B 组每周期计数步 ARR+1（0 = 未配置）
    pub n_b: u16,
    /// 各路实际占空比 0..=10000
    pub duties: [u16; 8],
}

impl ActualState {
    pub fn from_data(d: &[u8]) -> Result<Self, ParseError> {
        if d.len() < QUERY_DATA_LEN {
            return Err(ParseError::BadLength);
        }
        let be16 = |i: usize| u16::from_be_bytes([d[i], d[i + 1]]);
        let mut duties = [0u16; 8];
        for (i, slot) in duties.iter_mut().enumerate() {
            *slot = be16(9 + i * 2);
        }
        Ok(Self {
            flags: d[0],
            freq_a: be16(1),
            freq_b: be16(3),
            n_a: be16(5),
            n_b: be16(7),
            duties,
        })
    }

    /// bit0 急停锁存
    pub fn estop(&self) -> bool {
        self.flags & 0x01 != 0
    }
    /// bit1 已配置过
    pub fn configured(&self) -> bool {
        self.flags & 0x02 != 0
    }
    /// bit2 A 组运行中
    pub fn running_a(&self) -> bool {
        self.flags & 0x04 != 0
    }
    /// bit3 B 组运行中
    pub fn running_b(&self) -> bool {
        self.flags & 0x08 != 0
    }
}

/// `0x07` 版本查询回读
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VersionInfo {
    pub proto: u8,
    pub major: u8,
    pub minor: u8,
}

impl VersionInfo {
    pub fn from_data(d: &[u8]) -> Result<Self, ParseError> {
        if d.len() < 3 {
            return Err(ParseError::BadLength);
        }
        Ok(Self { proto: d[0], major: d[1], minor: d[2] })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 独立的按位参考实现，与被测实现交叉验证
    fn ref_crc8(data: &[u8]) -> u8 {
        let mut crc = 0u8;
        for &b in data {
            crc ^= b;
            for _ in 0..8 {
                crc = if crc & 0x80 != 0 { (crc << 1) ^ 0x07 } else { crc << 1 };
            }
        }
        crc
    }

    /// 按线上格式拼应答帧（测试辅助）
    fn resp_frame(cmd: u8, status: u8, data: &[u8]) -> Vec<u8> {
        let mut body = Vec::with_capacity(data.len() + 1);
        body.push(status);
        body.extend_from_slice(data);
        encode_frame(cmd | RESP_FLAG, &body)
    }

    fn feed_all(dec: &mut Decoder, bytes: &[u8], start_ms: u32) -> Vec<Event> {
        let mut out = Vec::new();
        for (i, &b) in bytes.iter().enumerate() {
            if let Some(ev) = dec.feed(b, start_ms + i as u32) {
                out.push(ev);
            }
        }
        out
    }

    #[test]
    fn crc_vectors() {
        assert_eq!(crc8(&[0x01, 0x02, 0x03, 0x04]), 0xE3);
        assert_eq!(crc8(&[0x04, 0x00]), 0x54);
        assert_eq!(crc8(&[0x02, 0x03, 0x00, 0x13, 0x88]), 0x27);
    }

    #[test]
    fn crc_matches_reference() {
        let data: Vec<u8> = (0..64).map(|i| (i * 37 + 11) as u8).collect();
        assert_eq!(crc8(&data), ref_crc8(&data));
        assert_eq!(crc8(&data[..1]), ref_crc8(&data[..1]));
        assert_eq!(crc8(&[]), 0x00);
    }

    #[test]
    fn encode_requests() {
        // 急停帧
        assert_eq!(Command::Estop.encode_request(), vec![0xAA, 0x55, 0x04, 0x00, 0x54]);
        // 单通道：ch0 占空比 5000（0x1388）
        assert_eq!(
            Command::SetDuty { channel: 0, duty: 5000 }.encode_request(),
            vec![0xAA, 0x55, 0x02, 0x03, 0x00, 0x13, 0x88, 0x27]
        );
        // 全量设置 20 字节载荷
        let f = Command::SetAll {
            freq_a: 0x0AE4,
            freq_b: 0x0AE4,
            duties: [5000, 0, 0, 0, 0, 0, 0, 0],
        }
        .encode_request();
        assert_eq!(f.len(), 2 + 1 + 1 + 20 + 1);
        assert_eq!(f[0..4], [0xAA, 0x55, 0x01, 0x14]);
        assert_eq!(f[4..6], [0x0A, 0xE4]);
        assert_eq!(f[8..10], [0x13, 0x88]);
        assert_eq!(*f.last().unwrap(), crc8(&f[2..f.len() - 1]));
    }

    #[test]
    fn decode_valid_responses() {
        let mut dec = Decoder::new();
        let frame = resp_frame(0x04, 0x00, &[]);
        let evs = feed_all(&mut dec, &frame, 0);
        assert_eq!(evs.len(), 1);
        match &evs[0] {
            Event::Frame(r) => {
                assert_eq!(r.cmd, 0x84);
                assert_eq!(r.status, Status::Ok);
                assert!(r.data.is_empty());
            }
            other => panic!("unexpected {other:?}"),
        }

        // 带数据应答
        let mut dec = Decoder::new();
        let frame = resp_frame(0x06, 0x00, &[0xAB; 25]);
        let evs = feed_all(&mut dec, &frame, 100);
        match &evs[0] {
            Event::Frame(r) => {
                assert_eq!(r.cmd, 0x86);
                assert_eq!(r.data.len(), 25);
            }
            other => panic!("unexpected {other:?}"),
        }

        // 状态码透传
        let mut dec = Decoder::new();
        let frame = resp_frame(0x01, 0x04, &[]);
        match &feed_all(&mut dec, &frame, 0)[0] {
            Event::Frame(r) => assert_eq!(r.status, Status::Estop),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn decode_resync() {
        // 帧前噪声
        let mut dec = Decoder::new();
        let mut bytes = vec![0x00, 0xFF, 0x55, 0x13];
        bytes.extend(resp_frame(0x04, 0x00, &[]));
        let evs = feed_all(&mut dec, &bytes, 0);
        assert_eq!(evs.len(), 1);
        assert!(matches!(evs[0], Event::Frame(_)));

        // AA AA 55 连帧头容忍
        let mut dec = Decoder::new();
        let mut bytes = vec![0xAA, 0xAA];
        bytes.extend(resp_frame(0x04, 0x00, &[]));
        let evs = feed_all(&mut dec, &bytes, 0);
        assert!(matches!(evs[0], Event::Frame(_)));

        // 坏 CRC：报错后可继续解析下一帧
        let mut dec = Decoder::new();
        let mut bytes = vec![0xAA, 0x55, 0x04, 0x00, 0x00];
        bytes.extend(resp_frame(0x04, 0x00, &[]));
        let evs = feed_all(&mut dec, &bytes, 0);
        assert_eq!(evs.len(), 2);
        assert_eq!(evs[0], Event::CrcError { cmd: 0x04 });
        assert!(matches!(evs[1], Event::Frame(_)));
    }

    #[test]
    fn decode_bad_len() {
        let mut dec = Decoder::new();
        let evs = feed_all(&mut dec, &[0xAA, 0x55, 0x01, 33], 0);
        assert_eq!(evs, vec![Event::LenError { cmd: 0x01 }]);
        // 后续帧正常
        let evs = feed_all(&mut dec, &resp_frame(0x04, 0x00, &[]), 10);
        assert!(matches!(evs[0], Event::Frame(_)));
    }

    #[test]
    fn decode_timeout() {
        // 半帧后间隙 6 ms（> TIMEOUT_MS）：残留被丢弃，新帧重新解析
        let mut dec = Decoder::new();
        assert!(feed_all(&mut dec, &[0xAA, 0x55, 0x01], 100).is_empty());
        let evs = feed_all(&mut dec, &resp_frame(0x04, 0x00, &[]), 108);
        assert!(matches!(evs[0], Event::Frame(_)));

        // 同一帧内分包，帧内间隙恰 5 ms（边界值）：不算超时
        let mut dec = Decoder::new();
        assert!(feed_all(&mut dec, &[0xAA, 0x55, 0x01], 100).is_empty());
        assert!(feed_all(&mut dec, &[0x00], 107).is_empty());
        let evs = feed_all(&mut dec, &[crc8(&[0x01, 0x00])], 108);
        match &evs[0] {
            Event::Frame(r) => assert_eq!(r.cmd, 0x01),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn parse_query_state() {
        let mut data = Vec::with_capacity(QUERY_DATA_LEN);
        data.push(0x0F); // 全部 flags
        data.extend_from_slice(&20000u16.to_be_bytes()); // freq_a
        data.extend_from_slice(&21000u16.to_be_bytes()); // freq_b
        data.extend_from_slice(&337u16.to_be_bytes()); // n_a
        data.extend_from_slice(&1105u16.to_be_bytes()); // n_b
        for i in 0..8u16 {
            data.extend_from_slice(&(i * 1000).to_be_bytes());
        }
        let st = ActualState::from_data(&data).unwrap();
        assert!(st.estop() && st.configured() && st.running_a() && st.running_b());
        assert_eq!(st.freq_a, 20000);
        assert_eq!(st.freq_b, 21000);
        assert_eq!(st.n_a, 337);
        assert_eq!(st.n_b, 1105);
        assert_eq!(st.duties, [0, 1000, 2000, 3000, 4000, 5000, 6000, 7000]);
        assert!(ActualState::from_data(&data[..24]).is_err());

        let v = VersionInfo::from_data(&[2, 1, 1]).unwrap();
        assert_eq!(v.proto, 2);
        assert!(VersionInfo::from_data(&[2, 1]).is_err());
    }
}

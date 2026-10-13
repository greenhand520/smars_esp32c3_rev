//! WS2812 灯带效果引擎（纯逻辑，host 可测）。
//!
//! 效果为逐帧渲染：给定时间与亮度，输出每颗灯的颜色，
//! 硬件驱动（`ui::Ws2812Strip`）负责把颜色帧发送到灯带。

/// RGB 颜色分量
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

/// 预设效果
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    /// 常亮（暖白）
    Solid,
    /// 呼吸（青色明暗渐变）
    Breath,
    /// 彩虹（整条色相流动）
    Rainbow,
    /// 流水（单点蓝色跑马）
    Chase,
}

impl Effect {
    /// 全部效果（网页按钮顺序）
    pub const ALL: [Effect; 4] = [Effect::Solid, Effect::Breath, Effect::Rainbow, Effect::Chase];

    /// 线上标识（/cmd 的 effect 字段）
    pub fn id(self) -> &'static str {
        match self {
            Effect::Solid => "solid",
            Effect::Breath => "breath",
            Effect::Rainbow => "rainbow",
            Effect::Chase => "chase",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "solid" => Some(Effect::Solid),
            "breath" => Some(Effect::Breath),
            "rainbow" => Some(Effect::Rainbow),
            "chase" => Some(Effect::Chase),
            _ => None,
        }
    }

    /// 显示名（网页按钮）
    pub fn name(self) -> &'static str {
        match self {
            Effect::Solid => "常亮",
            Effect::Breath => "呼吸",
            Effect::Rainbow => "彩虹",
            Effect::Chase => "流水",
        }
    }
}

/// 渲染一帧：`t_ms` 为效果运行时间（毫秒），`brightness` 为 0..=100。
/// `out` 长度即灯数（0 颗安全返回）。
pub fn render(effect: Effect, t_ms: u32, brightness: u8, out: &mut [Rgb]) {
    let b = (brightness.min(100) as u32) * 255 / 100;
    let n = out.len();
    if n == 0 {
        return;
    }
    match effect {
        Effect::Solid => {
            for c in out.iter_mut() {
                *c = scale(Rgb(255, 180, 90), b);
            }
        }
        Effect::Breath => {
            // 2 s 周期三角波 0..=255
            let phase = t_ms % 2000;
            let level = if phase < 1000 { phase * 255 / 1000 } else { (2000 - phase) * 255 / 1000 };
            let rgb = scale(Rgb(0, 180, 255), b);
            for c in out.iter_mut() {
                *c = Rgb(
                    (rgb.0 as u32 * level / 255) as u8,
                    (rgb.1 as u32 * level / 255) as u8,
                    (rgb.2 as u32 * level / 255) as u8,
                );
            }
        }
        Effect::Rainbow => {
            let base = (t_ms / 20) as u16;
            for (i, c) in out.iter_mut().enumerate() {
                let hue = ((i as u16) * 256 / n as u16 + base) as u8;
                *c = scale(hsv_to_rgb(hue), b);
            }
        }
        Effect::Chase => {
            let head = ((t_ms / 80) as usize) % n;
            for (i, c) in out.iter_mut().enumerate() {
                *c = if i == head { scale(Rgb(0, 90, 255), b) } else { Rgb(0, 0, 0) };
            }
        }
    }
}

/// 按亮度系数（0..=255 对应 0..=100%）缩放颜色
fn scale(rgb: Rgb, b: u32) -> Rgb {
    Rgb(
        (rgb.0 as u32 * b / 255) as u8,
        (rgb.1 as u32 * b / 255) as u8,
        (rgb.2 as u32 * b / 255) as u8,
    )
}

/// 色相（0..=255）→ RGB
fn hsv_to_rgb(h: u8) -> Rgb {
    let region = h / 43;
    let remainder = (h as u16 - region as u16 * 43) * 6; // 0..=252
    let up = (remainder / 3) as u8;
    let down = 255u16.saturating_sub(remainder / 3) as u8;
    match region {
        0 => Rgb(255, up, 0),
        1 => Rgb(down, 255, 0),
        2 => Rgb(0, 255, up),
        3 => Rgb(0, down, 255),
        4 => Rgb(up, 0, 255),
        _ => Rgb(255, 0, down),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effect_ids_roundtrip() {
        for e in Effect::ALL {
            assert_eq!(Effect::from_id(e.id()), Some(e));
        }
        assert_eq!(Effect::from_id("nope"), None);
    }

    #[test]
    fn render_zero_leds_safe() {
        render(Effect::Solid, 0, 100, &mut []);
    }

    #[test]
    fn brightness_zero_black() {
        for e in Effect::ALL {
            let mut out = [Rgb(1, 2, 3); 20];
            render(e, 1234, 0, &mut out);
            assert!(out.iter().all(|c| *c == Rgb(0, 0, 0)), "effect {:?}", e);
        }
    }

    #[test]
    fn solid_and_breath_uniform() {
        let mut out = [Rgb(0, 0, 0); 20];
        render(Effect::Solid, 0, 100, &mut out);
        assert!(out.iter().all(|c| *c == out[0] && *c != Rgb(0, 0, 0)));

        render(Effect::Breath, 0, 100, &mut out);
        assert!(out.iter().all(|c| *c == out[0]));
    }

    #[test]
    fn breath_cycles() {
        let mut a = [Rgb(0, 0, 0); 4];
        let mut b = [Rgb(0, 0, 0); 4];
        render(Effect::Breath, 0, 100, &mut a);    // 起点暗
        render(Effect::Breath, 1000, 100, &mut b); // 峰值亮
        assert_eq!(a[0], Rgb(0, 0, 0));
        assert!(b[0].1 > 100);
    }

    #[test]
    fn rainbow_varies_across_leds() {
        let mut out = [Rgb(0, 0, 0); 20];
        render(Effect::Rainbow, 0, 100, &mut out);
        assert_ne!(out[0], out[10]);
    }

    #[test]
    fn chase_single_lit_pixel_moves() {
        let mut out = [Rgb(0, 0, 0); 20];
        render(Effect::Chase, 0, 100, &mut out);
        let lit = out.iter().filter(|c| **c != Rgb(0, 0, 0)).count();
        assert_eq!(lit, 1);
        assert!(out[0] != Rgb(0, 0, 0));

        render(Effect::Chase, 80, 100, &mut out);
        assert!(out[1] != Rgb(0, 0, 0) && out[0] == Rgb(0, 0, 0));
    }

    #[test]
    fn brightness_scales() {
        let mut full = [Rgb(0, 0, 0); 4];
        let mut half = [Rgb(0, 0, 0); 4];
        render(Effect::Solid, 0, 100, &mut full);
        render(Effect::Solid, 0, 50, &mut half);
        assert!(half[0].0 < full[0].0 && half[0].0 > 0);
    }
}

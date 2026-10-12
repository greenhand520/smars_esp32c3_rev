//! 电池检测：BAT_ADC（IO3，ADC1_CH3）采样 + 滑动平均 + 电量换算。
//!
//! 前端 2:1 分压（design Q5），ADC 11 dB 衰减满量程约 2.5 V。
//! 开启 eFuse 校准（`calibration(true)`）后 `read()` 直接返回毫伏。

use esp_idf_svc::hal::adc::attenuation::{self, adc_atten_t};
use esp_idf_svc::hal::adc::{AdcChannelDriver, AdcDriver, ADC1, ADCCH3, ADCU1};
use esp_idf_svc::hal::gpio::Gpio3;
use esp_idf_svc::hal::sys::EspError;

/// 滑动平均窗口
const WINDOW: usize = 8;

/// 11 dB 衰减（满量程约 2.5 V，1S 分压后 2.1 V 在量程内）
const ATTEN: adc_atten_t = attenuation::DB_12;

type BatPin<'d> = AdcChannelDriver<'d, { ATTEN }, ADCCH3<ADCU1>>; // Gpio3 = ADC1_CH3

pub struct Battery<'d> {
    adc: AdcDriver<'d, ADCU1>,
    ch: BatPin<'d>,
    samples: [u32; WINDOW],
    idx: usize,
    filled: usize,
}

impl<'d> Battery<'d> {
    pub fn new(adc1: ADC1<'d>, pin: Gpio3<'d>) -> Result<Self, EspError> {
        let adc = AdcDriver::new(adc1, &esp_idf_svc::hal::adc::AdcConfig::new().calibration(true))?;
        let ch = AdcChannelDriver::new(pin)?;
        Ok(Self { adc, ch, samples: [0; WINDOW], idx: 0, filled: 0 })
    }

    /// 采一次样（电池电压 mV，含分压系数），内部滑动平均
    pub fn sample_mv(&mut self, scale_permille: u32) -> Option<u32> {
        // calibration(true) 下 read() 返回输入端毫伏
        let input_mv = self.adc.read(&mut self.ch).ok()? as u32;
        let batt_mv = smars_core::battery_mv(input_mv, scale_permille);

        self.samples[self.idx] = batt_mv;
        self.idx = (self.idx + 1) % WINDOW;
        if self.filled < WINDOW {
            self.filled += 1;
        }
        let sum: u32 = self.samples[..self.filled].iter().sum();
        Some(sum / self.filled as u32)
    }
}

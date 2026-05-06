use az::SaturatingCast;
use embassy_stm32::adc::{Resolution, resolution_to_max_count};
use libm::logf;
use num_traits::float::FloatCore;

const R_REF: f32 = 10_000.0;
const KELVIN: f32 = 273.15;

#[derive(Debug, Clone)]
pub struct Ntc {
    pub beta: f32,
    pub t0_deg: f32,
    pub r0: f32,
}

impl Ntc {
    pub const fn build(self, resolution: Resolution) -> NtcReader {
        NtcReader {
            beta: self.beta,
            rev_t0: 1.0 / (self.t0_deg + KELVIN),
            r_fac: R_REF / self.r0,
            max: resolution_to_max_count(resolution) as f32,
        }
    }
}

pub struct NtcReader {
    beta: f32,
    rev_t0: f32, // 1/T0
    r_fac: f32, // = R_REF/R0
    max: f32,
}

impl NtcReader {
    /// Convert raw ADC reading into temperature in 1/1000 °C.
    pub fn from_adc(&self, reading: u16) -> i16 {
        let ratio = self.max / reading as f32;
        (self.ratio2temperature(ratio) * 1000.0).round().saturating_cast()
    }

    #[inline]
    fn ratio2temperature(&self, reading_rel_rev: f32) -> f32 {
        let r_fac = self.r_fac / ((self.r_fac + 1.0) * reading_rel_rev - 1.0);
        let rev_t = self.rev_t0 + logf(r_fac) / self.beta;
        (1.0 / rev_t) - KELVIN
    }
}

//! ADC driver reading battery voltage and control pin voltage

use array_macro::array;
use core::{
    ptr,
    sync::atomic::{AtomicI16, AtomicU16, Ordering},
};
use defmt::{debug, info};
use embassy_executor::task;
use embassy_stm32::{
    adc::{resolution_to_max_count, Adc, AdcChannel, AnyAdcChannel, Resolution, SampleTime, VDDA_CALIB_MV},
    gpio::Output,
    peripherals::ADC1,
};
use embassy_time::Timer;

const VOLT_FACTOR: u32 = 10;
const RESOLUTION: Resolution = Resolution::BITS12;
const SAMPLE_TIME: SampleTime = SampleTime::CYCLES239_5;

pub static CPU_TEMPERATURE: AtomicI16 = AtomicI16::new(0);
pub static CURRENTS: [AtomicU16; 4] = array![_ => AtomicU16::new(0); 4];

fn get_vref_cal() -> u32 {
    unsafe {
        // DocID025832 Rev. 5
        ptr::read_volatile(0x1FFF_F7BA as *const u16) as u32
    }
}

fn get_ts_cal() -> (i32, i32) {
    unsafe {
        // DocID025832 Rev. 5
        (
            ptr::read_volatile(0x1FFF_F7B8 as *const u16) as i32,
            ptr::read_volatile(0x1FFF_F7C2 as *const u16) as i32,
        )
    }
}

/// ADC input pins.
pub struct AdcReader {
    pub adc: Adc<'static, ADC1>,
    pub ts: [AnyAdcChannel<'static, ADC1>; 4],
    pub vsense: AnyAdcChannel<'static, ADC1>,
    pub isense: AnyAdcChannel<'static, ADC1>,
}

impl AdcReader {
    pub fn init(mut self) -> AdcReaderState {
        let vref_cal = get_vref_cal();
        let (t30_cal, t110_cal) = get_ts_cal();
        self.adc.set_resolution(RESOLUTION);

        let reference = self.adc.enable_vref().degrade_adc();
        let tempsensor = self.adc.enable_temperature().degrade_adc();
        info!("ADC calibration value = {}", vref_cal);
        info!("T calibration values = {}, {}", t30_cal, t110_cal);

        let max = resolution_to_max_count(RESOLUTION);

        let [ts0, ts1, ts2, ts3] = self.ts;
        let inputs = [
            ts0, ts1, ts2, ts3,
            tempsensor, reference, self.isense, self.vsense,
        ];

        AdcReaderState {
            adc: self.adc,
            inputs,
            vref_cal,
            t30_cal,
            t110_cal,
            max,
        }
    }
}

pub struct AdcReaderState {
    adc: Adc<'static, ADC1>,
    inputs: [AnyAdcChannel<'static, ADC1>; 8],
    vref_cal: u32,
    t30_cal: i32,
    t110_cal: i32,
    max: u32,
}

#[repr(usize)]
enum Idx {
    Ts = 0,
    TempSensor = 4,
    Reference = 5,
    ISense = 6,
    VSense = 7,
}


impl AdcReaderState {
    pub async fn read(&mut self) {
        let mut readings: [u16; 8] = [0; 8];
        for (inp, out) in self.inputs.iter_mut().zip(readings.iter_mut()) {
            *out = self.adc.read(inp, SAMPLE_TIME).await;
        }

        let chip_temperature = readings[Idx::TempSensor as usize];
        let vref = readings[Idx::Reference as usize];

        // RM0091 13.8 Calculating the actual VDDA voltage using the internal reference voltage
        // V_DDA = 3.3 V x VREFINT_CAL / VREFINT_DATA
        let vdda = (self.vref_cal * VDDA_CALIB_MV) / vref as u32;

        // RM0091 13.8 Reading the temperature
        // T = (110 °C - 30 °C) / (TS_CAL2 - TS_CAL1) × (TS_DATA - TS_CAL1) + 30 °C
        let ts = chip_temperature as i32 * 3300 / vdda as i32;
        let chip_temperature = ((ts - self.t30_cal) * (110 - 30) / (self.t110_cal - self.t30_cal) + 30) as i16;

        let ts = readings[Idx::Ts as usize .. Idx::Ts as usize + 4].iter().map(|ts| {
        });
        //let sense_voltage_mv = (voltage as u32 * vdda / max * VOLT_FACTOR) as u16;
        //let current_ma = sense_voltage_mv * 2;

    }
}

//! ADC driver reading battery voltage and control pin voltage

use core::{
    ptr,
    sync::atomic::{AtomicI16, AtomicU16, Ordering},
};
use embassy_executor::task;
use embassy_stm32::{
    adc::{resolution_to_max_count, Adc, AnyAdcChannel, Resolution, SampleTime, VDDA_CALIB_MV},
    peripherals::ADC1,
};
use embassy_time::{Duration, Timer};

use crate::SHUTDOWN;

const BATT_LOW_THRESHOLD_MV: u16 = 12200;
const CONTROL_LOW_THRESHOLD_MV: u16 = 10000;

const VOLT_FACTOR: u32 = 10;
const RESOLUTION: Resolution = Resolution::BITS12;

pub static BATTERY_VOLTAGE_MV: AtomicU16 = AtomicU16::new(u16::MAX);
pub static CONTROL_VOLTAGE_MV: AtomicU16 = AtomicU16::new(0);
pub static CPU_TEMPERATURE: AtomicI16 = AtomicI16::new(0);

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

#[task]
pub async fn process(
    mut adc: Adc<'static, ADC1>,
    mut pin_batt_voltage: AnyAdcChannel<'static, ADC1>,
    mut pin_control_voltage: AnyAdcChannel<'static, ADC1>,
) {
    const SAMPLE_TIME: SampleTime = SampleTime::CYCLES239_5;

    let vref_cal = get_vref_cal();
    let (t30_cal, t110_cal) = get_ts_cal();
    adc.set_resolution(RESOLUTION);

    let mut reference = adc.enable_vref();
    let mut tempsensor = adc.enable_temperature();
    let max = resolution_to_max_count(RESOLUTION);
    loop {
        let voltage = adc.read(&mut pin_batt_voltage, SAMPLE_TIME).await;
        let control = adc.read(&mut pin_control_voltage, SAMPLE_TIME).await;
        let vref = adc.read(&mut reference, SAMPLE_TIME).await;
        let temperature = adc.read(&mut tempsensor, SAMPLE_TIME).await;

        // RM0091 13.8 Calculating the actual VDDA voltage using the internal reference voltage
        // V_DDA = 3.3 V x VREFINT_CAL / VREFINT_DATA
        let vdda = (vref_cal * VDDA_CALIB_MV) / vref as u32;

        // RM0091 13.8 Reading the temperature
        // T = (110 °C - 30 °C) / (TS_CAL2 - TS_CAL1) × (TS_DATA - TS_CAL1) + 30 °C
        let ts = temperature as i32 * 3300 / vdda as i32;
        let temperature = ((ts - t30_cal) * (110 - 30) / (t110_cal - t30_cal) + 30) as i16;

        let battery_voltage_mv = (voltage as u32 * vdda / max * VOLT_FACTOR) as u16;
        let control_voltage_mv = (control as u32 * vdda / max * VOLT_FACTOR) as u16;

        if control_voltage_mv < CONTROL_LOW_THRESHOLD_MV
            || battery_voltage_mv < BATT_LOW_THRESHOLD_MV
        {
            SHUTDOWN.store(true, Ordering::Relaxed);
        }

        CONTROL_VOLTAGE_MV.store(control_voltage_mv, Ordering::Relaxed);
        BATTERY_VOLTAGE_MV.store(battery_voltage_mv, Ordering::Relaxed);
        CPU_TEMPERATURE.store(temperature, Ordering::Relaxed);
        Timer::after(Duration::from_millis(100)).await;
    }
}

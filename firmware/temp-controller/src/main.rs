#![feature(impl_trait_in_assoc_type)]
#![no_std]
#![no_main]

mod adc;
mod can;
mod ntc;
mod pwm;
mod tbh;
mod temperature;
mod traits;

mod power_regulator;

use {defmt_rtt as _, panic_probe as _};

use crate::{
    adc::AdcReader, can::process as can_process, pwm::Pwm,
    temperature::process as temperature_process,
    tbh::Tbh,
    power_regulator::PowerRegulator,
    traits::{AsInput, Regulator},
};

use core::{
    cell::RefCell,
    sync::atomic::{AtomicBool, AtomicU8, Ordering},
};
use defmt::{info, unwrap, Debug2Format};
use embassy_embedded_hal::shared_bus::asynch::i2c::I2cDevice;
use embassy_executor::{main, task, Spawner};
use embassy_futures::{yield_now, select::select};
use embassy_stm32::{
    adc::{self as stm32_adc, Adc, AdcChannel},
    bind_interrupts,
    can::{self as stm32_can, Can},
    dma,
    exti::{self, ExtiInput},
    gpio::{Flex, Input, Level, Output, OutputOpenDrain, OutputType, Pull, Speed},
    i2c::{self, Config as I2cConfig, I2c},
    mode::Async,
    pac, peripherals,
    time::{khz, mhz},
    timer::{
        low_level::{CountingMode, OutputPolarity},
        simple_pwm::{PwmPin, SimplePwm},
    },
    wdg::IndependentWatchdog,
    Config as DeviceConfig,
};
use embassy_sync::{blocking_mutex::raw::NoopRawMutex, mutex::Mutex};
use embassy_time::{Instant, Timer};
use static_cell::StaticCell;

bind_interrupts!(struct Irqs {
    I2C1 => i2c::EventInterruptHandler<peripherals::I2C1>, i2c::ErrorInterruptHandler<peripherals::I2C1>;
    ADC1 => stm32_adc::InterruptHandler<peripherals::ADC1>;
    CEC_CAN => stm32_can::Rx0InterruptHandler<peripherals::CAN>, stm32_can::Rx1InterruptHandler<peripherals::CAN>,
               stm32_can::TxInterruptHandler<peripherals::CAN>, stm32_can::SceInterruptHandler<peripherals::CAN>;
    DMA1_CHANNEL2_3 => dma::InterruptHandler<peripherals::DMA1_CH2>, dma::InterruptHandler<peripherals::DMA1_CH3>;
});

static PWM_DUTY: AtomicU8 = AtomicU8::new(0);

#[main]
async fn main(spawner: Spawner) {
    // HSI oscillator 12 MHz, 64 MHz system frequency
    let mut config = DeviceConfig::default();
    {
        use embassy_stm32::rcc::*;
        config.rcc.hsi = true;
        config.rcc.hse = None;
        config.rcc.pll = Some(Pll {
            src: PllSource::HSI,
            prediv: PllPreDiv::DIV1,
            mul: PllMul::MUL6,
        });
        config.rcc.sys = Sysclk::PLL1_P;
        config.rcc.ahb_pre = AHBPrescaler::DIV1;
        config.rcc.apb1_pre = APBPrescaler::DIV1;
        config.enable_debug_during_sleep = true;
    }
    let dev = embassy_stm32::init(config);

    // Reconfigure pins for CAN bus
    pac::SYSCFG.cfgr1().modify(|w| w.set_pa11_pa12_rmp(true));

    let sda = dev.PF0;
    let scl = dev.PF1;
    let i2c = I2c::new(dev.I2C1, scl, sda, dev.DMA1_CH2, dev.DMA1_CH3, Irqs, {
        let mut cfg = I2cConfig::default();
        cfg.frequency = khz(400);
        cfg
    });

    let adc = Adc::new(dev.ADC1, Irqs);
    let adc = AdcReader {
        adc,
        ts: [
            dev.PA0.degrade_adc(),
            dev.PA1.degrade_adc(),
            dev.PA2.degrade_adc(),
            dev.PA3.degrade_adc(),
        ],
        vsense: dev.PA5.degrade_adc(),
        isense: dev.PA4.degrade_adc(),
    };
    let mut adc = adc.init();

    //    unwrap!(spawner.spawn(temperature_process(i2c)));

    let can = Can::new(dev.CAN, dev.PA11, dev.PA12, Irqs);
    //spawner.spawn(can_process(can).unwrap());

    let pwm = PwmPin::new(dev.PA7, OutputType::PushPull);
    let pwm = SimplePwm::new(
        dev.TIM3,
        None,
        Some(pwm),
        None,
        None,
        khz(400),
        CountingMode::EdgeAlignedUp,
    );
    let mut pwm = pwm.split().ch2;
    pwm.set_polarity(OutputPolarity::ActiveHigh);
    pwm.set_duty_cycle_fully_off();
    pwm.enable();

    let drv_en = Output::new(dev.PB8, Level::Low, Speed::Low);

    let pwm = Pwm::new(pwm, drv_en, 0.1);

    let mut led = OutputOpenDrain::new(dev.PA6, Level::High, Speed::Low);

    let power_regulator = PowerRegulator::new(pwm);
    let mut temperature_regulator = Tbh::new(0.0005, power_regulator, 10.0);

    loop {
        led.toggle();
        let t = Instant::now();
        let reading = adc.read().await;
        let temperature = reading.ts[0] as f32 / 1_000.0;
        info!("T = {}", temperature);
        let power = reading.output_current_ma as f32 * reading.output_voltage_mv as f32 / 1_000_000.0;
        let input = Inputs {
            power,
            temperature,
        };
        temperature_regulator.regulate(t, &input, 45.0);
        Timer::after_millis(200).await;
    }
}

struct Inputs {
    power: f32,
    temperature: f32,
}

impl<T> AsInput<PowerRegulator<T>> for Inputs {
    type Value = f32;
    fn as_input(&self) -> Self::Value {
        self.power
    }
}

impl<T> AsInput<Tbh<T>> for Inputs {
    type Value = f32;
    fn as_input(&self) -> Self::Value {
        self.temperature
    }
}

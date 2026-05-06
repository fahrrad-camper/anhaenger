#![feature(impl_trait_in_assoc_type)]
#![no_std]
#![no_main]

mod adc;
mod ntc;
mod temperature;
mod can;

use {defmt_rtt as _, panic_probe as _};

use crate::{adc::AdcReader, temperature::process as temperature_process, can::process as can_process};
use core::{
    cell::RefCell,
    sync::atomic::{AtomicBool, AtomicU8, Ordering},
};
use defmt::{info, unwrap};
use embassy_embedded_hal::shared_bus::asynch::i2c::I2cDevice;
use embassy_executor::{main, task, Spawner};
use embassy_futures::select::select;
use embassy_stm32::{
    pac,
    adc::{self as stm32_adc, Adc, AdcChannel},
    bind_interrupts,
    can::{self as stm32_can, Can},
    exti::{self, ExtiInput},
    dma,
    gpio::{Flex, Input, Level, Output, OutputOpenDrain, OutputType, Pull, Speed},
    i2c::{self, I2c, Config as I2cConfig},
    mode::Async,
    peripherals,
    time::{khz, mhz},
    timer::{
        low_level::{CountingMode, OutputPolarity},
        simple_pwm::{PwmPin, SimplePwm},
    },
    wdg::IndependentWatchdog,
    Config as DeviceConfig,
};
use embassy_sync::{blocking_mutex::raw::NoopRawMutex, mutex::Mutex};
use embassy_time::Timer;
use num_traits::float::FloatCore;
use pid::Pid;
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
    }
    let dev = embassy_stm32::init(config);

    // Reconfigure pins for CAN bus
    pac::SYSCFG.cfgr1().modify(|w| w.set_pa11_pa12_rmp(true));

    let sda = dev.PF0;
    let scl = dev.PF1;
    let i2c = I2c::new(
        dev.I2C1,
        scl,
        sda,
        dev.DMA1_CH2,
        dev.DMA1_CH3,
        Irqs,
        {
            let mut cfg = I2cConfig::default();
            cfg.frequency = khz(400);
            cfg
        },
    );


    let adc = Adc::new(dev.ADC1, Irqs);
    let adc = AdcReader {
        adc,
        ts: [dev.PA0.degrade_adc(), dev.PA1.degrade_adc(), dev.PA2.degrade_adc(), dev.PA3.degrade_adc()],
        vsense: dev.PA5.degrade_adc(),
        isense: dev.PA4.degrade_adc(),
    };
    let adc = adc.init();

//    unwrap!(spawner.spawn(temperature_process(i2c)));

    let can = Can::new(dev.CAN, dev.PA11, dev.PA12, Irqs);
    spawner.spawn(can_process(can).unwrap());

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

    let mut drv_en = Output::new(dev.PB8, Level::Low, Speed::Low);

    let mut led = OutputOpenDrain::new(dev.PA6, Level::High, Speed::Low);

  //  let mut pid = Pid::<f32>::new(15.0, 100.0);
  //  pid.p(10.0, 100.0).i(0.1, 50.0).d(0.1, 10.0);

    drv_en.set_high();
    loop {
        Timer::after_millis(100).await;
        led.set_high();
        Timer::after_millis(100).await;
        led.set_low();
        pwm.set_duty_cycle_fraction(1, 20);
/*        let t = temperature::TEMPERATURE.load(Ordering::Relaxed) as f32 / 10.0;
        let v = pid.next_control_output(t);

        let duty = (-v.output).clamp(0.0, 100.0);
        PWM_DUTY.store(duty.round() as u8, Ordering::Relaxed);
        info!("PWM duty {}", duty);

        pwm.ch1
            .set_duty_cycle_fraction((duty * 100.0).round() as u16, 10000);*/
    }
}

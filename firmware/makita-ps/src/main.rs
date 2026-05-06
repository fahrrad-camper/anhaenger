#![feature(impl_trait_in_assoc_type)]
#![no_std]
#![no_main]

mod adc;
mod can;
mod display;
mod led;
mod vmon;

use {defmt_rtt as _, panic_probe as _};

use crate::{
    adc::process as adc_process,
    can::process as can_process,
    display::process as display_process,
    led::{Color, Led},
    vmon::process as voltage_monitor_process,
};
use core::sync::atomic::{AtomicBool, Ordering};
use defmt::info;
use embassy_executor::{main, task, Spawner};
use embassy_futures::{join::join, select::select};
use embassy_stm32::{
    adc::{self as stm32_adc, Adc, AdcChannel},
    bind_interrupts,
    can::{self as stm32_can, Can},
    dma,
    exti::{self, ExtiInput},
    gpio::{Flex, Input, Level, Output, Pull, Speed},
    i2c::{self, mode::Master, Config as I2cConfig, I2c},
    interrupt,
    mode::Async,
    pac, peripherals,
    time::khz,
    wdg::IndependentWatchdog,
    Config as DeviceConfig,
};
use embassy_sync::{blocking_mutex::raw::NoopRawMutex, mutex::Mutex};
use embassy_time::{Duration, Timer};
use static_cell::StaticCell;

bind_interrupts!(struct Irqs {
    I2C1 => i2c::EventInterruptHandler<peripherals::I2C1>, i2c::ErrorInterruptHandler<peripherals::I2C1>;
    DMA1_CHANNEL2_3 => dma::InterruptHandler<peripherals::DMA1_CH2>, dma::InterruptHandler<peripherals::DMA1_CH3>;
    ADC1 => stm32_adc::InterruptHandler<peripherals::ADC1>;
    CEC_CAN => stm32_can::Rx0InterruptHandler<peripherals::CAN>, stm32_can::Rx1InterruptHandler<peripherals::CAN>,
               stm32_can::TxInterruptHandler<peripherals::CAN>, stm32_can::SceInterruptHandler<peripherals::CAN>;
    EXTI4_15 => exti::InterruptHandler<interrupt::typelevel::EXTI4_15>;
});

static WANT_12V: AtomicBool = AtomicBool::new(false);
static SHUTDOWN: AtomicBool = AtomicBool::new(false);

#[task]
async fn power_process(mut btn_sense: ExtiInput<'static, Async>) {
    loop {
        btn_sense.wait_for_rising_edge().await;
        select(
            async {
                btn_sense.wait_for_low().await;
            },
            async {
                Timer::after(Duration::from_millis(1000)).await;
                SHUTDOWN.store(true, Ordering::Relaxed);
            },
        )
        .await;
    }
}

#[task]
async fn delayed_12v_on() {
    Timer::after(Duration::from_secs(1)).await;
    info!("Turning on 12V");
    WANT_12V.store(true, Ordering::Relaxed);
}

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

    // Turn software power switch on now
    let mut pwr_enable = Flex::new(dev.PB8);
    pwr_enable.set_high();
    pwr_enable.set_as_output(Speed::Low);

    // Configure watchdog
    let mut dog = IndependentWatchdog::new(dev.IWDG, 100_000);
    dog.unleash();

    // RGB LED
    let mut led = Led::new(dev.PA6, dev.PA7, dev.PB1);
    led.set_color(Color::Magenta);

    // 12V control
    let pg_12v = Input::new(dev.PA2, Pull::Up);
    let mut en_12v = Output::new(dev.PA3, Level::Low, Speed::Low);

    // Power on-off switch
    let pwr_btn_sense = ExtiInput::new(dev.PA4, dev.EXTI4, Pull::Down, Irqs);
    spawner.spawn(power_process(pwr_btn_sense).unwrap());
    dog.pet();

    // ADC for battery monitoring
    // VMON_BAT PA0
    // SMON_BAT PA1
    let adc = Adc::new(dev.ADC1, Irqs);
    spawner.spawn(adc_process(adc, dev.PA0.degrade_adc(), dev.PA1.degrade_adc()).unwrap());

    // AUX PA5
    // CAN_RX PA9/PA11
    // CAN_TX PA10/PA12

    // I²C bus
    let scl = dev.PF1;
    let sda = dev.PF0;
    let i2c = I2c::new(dev.I2C1, scl, sda, dev.DMA1_CH2, dev.DMA1_CH3, Irqs, {
        let mut cfg = I2cConfig::default();
        cfg.frequency = khz(400);
        cfg
    });
    static I2C_BUS: StaticCell<Mutex<NoopRawMutex, I2c<'_, Async, Master>>> = StaticCell::new();
    let i2c = Mutex::new(i2c);
    let i2c = I2C_BUS.init(i2c);

    spawner.spawn(voltage_monitor_process(i2c).unwrap());
    dog.pet();

    spawner.spawn(display_process(i2c).unwrap());
    dog.pet();

    let can = Can::new(dev.CAN, dev.PA11, dev.PA12, Irqs);
    spawner.spawn(can_process(can).unwrap());

    info!("System startup");
    spawner.spawn(delayed_12v_on().unwrap());
    while !SHUTDOWN.load(Ordering::Relaxed) {
        if pg_12v.is_low() {
            led.set_color(Color::Blue);
        } else {
            led.set_color(Color::Green);
        }

        if WANT_12V.load(Ordering::Relaxed) {
            en_12v.set_high();
        } else {
            en_12v.set_low();
        }

        dog.pet();
        Timer::after(Duration::from_millis(1)).await;
    }

    info!("Powering down");
    join(
        async {
            for _ in 0..3 {
                led.set_color(Color::Red);
                Timer::after(Duration::from_millis(150)).await;
                led.set_color(Color::Off);
                Timer::after(Duration::from_millis(150)).await;
            }
            pwr_enable.set_as_input(Pull::None);
        },
        async {
            loop {
                dog.pet();
                Timer::after_millis(10).await;
            }
        },
    )
    .await;
}

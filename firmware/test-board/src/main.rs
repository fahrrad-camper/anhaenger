#![feature(impl_trait_in_assoc_type)]
#![no_std]
#![no_main]

mod display;

use {defmt_rtt as _, panic_probe as _};

use can_messages::{prelude::*, BatteryData, CoolBox, PowerOff, BITRATE};
use defmt::{info, Debug2Format};
use embassy_embedded_hal::shared_bus::asynch::i2c::I2cDevice;
use embassy_executor::{main, task, Spawner};
use embassy_stm32::{
    bind_interrupts,
    can::{self as stm32_can, filter::Mask32, Can, CanTx, Fifo},
    dma,
    exti::{self, ExtiInput},
    gpio::Pull,
    i2c::{self, mode::Master, Config as I2cConfig, I2c},
    interrupt,
    mode::Async,
    pac, peripherals,
    time::khz,
    usart::{self, Config as UartConfig, DataBits, Parity, StopBits, Uart},
    Config as DeviceConfig,
};
use embassy_sync::{blocking_mutex::raw::NoopRawMutex, mutex::Mutex};
use embassy_time::{Duration, Instant};
use heapless::format;
use static_cell::StaticCell;

bind_interrupts!(struct Irqs {
    I2C1 => i2c::EventInterruptHandler<peripherals::I2C1>, i2c::ErrorInterruptHandler<peripherals::I2C1>;
    CEC_CAN => stm32_can::Rx0InterruptHandler<peripherals::CAN>, stm32_can::Rx1InterruptHandler<peripherals::CAN>,
               stm32_can::TxInterruptHandler<peripherals::CAN>, stm32_can::SceInterruptHandler<peripherals::CAN>;
    USART2 => usart::InterruptHandler<peripherals::USART2>;
    EXTI4_15 => exti::InterruptHandler<interrupt::typelevel::EXTI4_15>;
    DMA1_CHANNEL2_3 => dma::InterruptHandler<peripherals::DMA1_CH2>, dma::InterruptHandler<peripherals::DMA1_CH3>;
    DMA1_CHANNEL4_5 => dma::InterruptHandler<peripherals::DMA1_CH4>, dma::InterruptHandler<peripherals::DMA1_CH5>;
});

#[task]
async fn send_poweroff(mut tx: CanTx<'static>, mut btn: ExtiInput<'static, Async>) {
    loop {
        btn.wait_for_falling_edge().await;
        tx.write(&PowerOff.try_encode().unwrap()).await;
    }
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

    // Button
    let btn = ExtiInput::new(dev.PA6, dev.EXTI6, Pull::Up, Irqs);

    // I²C bus
    let scl = dev.PF1;
    let sda = dev.PF0;
    let i2c = I2c::new(dev.I2C1, scl, sda, dev.DMA1_CH2, dev.DMA1_CH3, Irqs, {
        let mut cfg = I2cConfig::default();
        cfg.frequency = khz(400);
        cfg.sda_pullup = true;
        cfg.scl_pullup = true;
        cfg
    });

    static I2C_BUS: StaticCell<Mutex<NoopRawMutex, I2c<'_, Async, Master>>> = StaticCell::new();
    let i2c = Mutex::new(i2c);
    let i2c = I2C_BUS.init(i2c);

    spawner.spawn(display::process(I2cDevice::new(i2c)).unwrap());

    let mut can = Can::new(dev.CAN, dev.PA11, dev.PA12, Irqs);
    can.set_bitrate(BITRATE);
    can.set_tx_fifo_scheduling(true);
    can.enable().await;
    info!("CAN initialized.");
    let (tx, mut rx) = can.split();

    let uart = Uart::new(
        dev.USART2,
        dev.PA3,
        dev.PA2,
        dev.DMA1_CH4,
        dev.DMA1_CH5,
        Irqs,
        {
            let mut cfg = UartConfig::default();
            cfg.baudrate = 1_000_000;
            cfg.data_bits = DataBits::DataBits8;
            cfg.stop_bits = StopBits::STOP1;
            cfg.parity = Parity::ParityNone;
            cfg
        },
    )
    .expect("Error initializing UART");

    let (mut uart_tx, _uart_rx) = uart.split();

    rx.modify_filters()
        .enable_bank(0, Fifo::Fifo0, Mask32::accept_all());

    spawner.spawn(send_poweroff(tx, btn).unwrap());

    info!("System startup");

    let mut batt_ts = Instant::MIN;
    let mut cool_ts = Instant::MIN;
    loop {
        if let Ok(msg) = rx.read().await {
            let now = Instant::now();
            if let Some(batt) = msg.try_decode::<BatteryData>() {
                info!("CAN battery: {}", Debug2Format(&batt));
                display::BATT_SIGNAL.signal(batt.clone());

                if now - batt_ts > Duration::from_secs(5) {
                    batt_ts = now;
                    let buf = format!(64; ":BATT {}\n:OUTP {}\n:CURR {}\n", batt.battery_voltage_mv, batt.output_voltage_mv, batt.output_current_ma).unwrap();
                    let _ = uart_tx.write(buf.as_bytes()).await;
                }
            } else if let Some(cool) = msg.try_decode::<CoolBox>() {
                info!("CAN coolbox: {}", Debug2Format(&cool));
                display::COOL_SIGNAL.signal(cool.clone());

                if now - cool_ts > Duration::from_secs(5) {
                    cool_ts = now;
                    let buf = format!(64; ":BOXT {}\n", cool.box_temperature_deg10).unwrap();
                    let _ = uart_tx.write(buf.as_bytes()).await;
                }
            } else {
                info!("CAN message received: {}", Debug2Format(&msg));
            }
        }
    }
}

use can_messages::{BatteryData, CoolBox};
use embassy_embedded_hal::shared_bus::asynch::i2c::I2cDevice;
use embassy_executor::task;
use embassy_futures::select::select;
use embassy_stm32::{
    i2c::{I2c, Master},
    mode::Async,
};
use embassy_sync::{
    blocking_mutex::raw::{NoopRawMutex, ThreadModeRawMutex},
    signal::Signal,
};
use embassy_time::Timer;
use heapless::format;
use ssd1306::{prelude::*, I2CDisplayInterface, Ssd1306Async};

pub static BATT_SIGNAL: Signal<ThreadModeRawMutex, BatteryData> = Signal::new();
pub static COOL_SIGNAL: Signal<ThreadModeRawMutex, CoolBox> = Signal::new();

#[task]
pub async fn process(i2c: I2cDevice<'static, NoopRawMutex, I2c<'static, Async, Master>>) {
    let iface = I2CDisplayInterface::new(i2c);
    let mut display =
        Ssd1306Async::new(iface, DisplaySize128x64, DisplayRotation::Rotate0).into_terminal_mode();

    for _ in 0..10 {
        let r = display.init().await;
        if r.is_ok() {
            break;
        }
        Timer::after_millis(10).await;
    }

    let _ = display.clear().await;
    let _ = display.write_str("It works!").await;

    let mut batt_value: Option<BatteryData> = None;
    let mut cool_value: Option<CoolBox> = None;

    loop {
        select(
            async {
                let batt = BATT_SIGNAL.wait().await;
                batt_value = Some(batt);
            },
            async {
                let cool = COOL_SIGNAL.wait().await;
                cool_value = Some(cool);
            },
        )
        .await;

        if let Some(batt) = batt_value.as_ref() {
            let _ = display.set_position(0, 0).await;
            let buf = format!(64; "Bat: {:>5} mV", batt.battery_voltage_mv).unwrap();
            let _ = display.write_str(&buf).await;
        }

        if let Some(cool) = cool_value.as_ref() {
            let _ = display.set_position(0, 1).await;
            let buf = format!(64; "Temp: {:>5} /10C\nDuty: {:>3}%", cool.box_temperature_deg10, cool.pwm_duty).unwrap();
            let _ = display.write_str(&buf).await;
        }
    }
}

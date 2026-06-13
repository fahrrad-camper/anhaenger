use core::sync::atomic::{AtomicI16, AtomicU16, Ordering};
use defmt::{debug, info};
use embassy_executor::task;
use embassy_stm32::{
    i2c::{mode::Master, I2c},
    mode::Async,
};
use embassy_time::{Delay, Timer};
use hdc1080_async::Hdc1080;

use embedded_hal_async::i2c::I2c as I2cAsync;

use defmt::Debug2Format;

pub static TEMPERATURE: AtomicI16 = AtomicI16::new(200);

#[task]
pub async fn process(i2c: I2c<'static, Async, Master>) {
    Timer::after_millis(1000).await;

    info!("Initializing temperature reading");
    let mut sensor = Hdc1080::new(i2c, Delay);
    let id = sensor
        .identify_async()
        .await
        .expect("Can't communicate with sensor");
    info!("Sensor ID: {:?}", Debug2Format(&id));
    info!(
        "Sensor ID is {}valid.",
        if id.is_valid() { "" } else { "NOT " }
    );

    sensor.reset_async().await.expect("Sensor reset fail");

    loop {
        Timer::after_millis(100).await;
        let (t, h) = sensor.read_async().await.expect("Sensor failure");
        info!("T = {}  H = {}", t.degrees_10(), h.percent_10());
        TEMPERATURE.store(t.degrees_10(), Ordering::Relaxed);
    }
}

use crate::{
    adc::BATTERY_VOLTAGE_MV,
    vmon::{OUTPUT_CURRENT_MA, OUTPUT_VOLTAGE_MV},
};
use can_messages::{prelude::*, BatteryData, CanId, PowerOff, BITRATE};
use core::sync::atomic::Ordering;
use defmt::info;
use embassy_executor::task;
use embassy_futures::join::join;
use embassy_stm32::can::{filter::Mask32, Can, CanRx, CanTx, Fifo, StandardId};
use embassy_time::Timer;

#[task]
pub async fn process(mut can: Can<'static>) {
    can.set_bitrate(BITRATE);
    can.set_tx_fifo_scheduling(true);
    can.enable().await;
    info!("CAN initialized.");
    let (tx, rx) = can.split();
    join(transmit(tx), receive(rx)).await;
}

async fn receive(mut rx: CanRx<'static>) {
    let filter = Mask32::frames_with_std_id(
        StandardId::new(CanId::POWEROFF.into()).unwrap(),
        StandardId::MAX,
    );
    rx.modify_filters().enable_bank(0, Fifo::Fifo0, filter);
    loop {
        if let Ok(msg) = rx.read().await {
            info!("CAN message received");

            if let Some(PowerOff) = msg.try_decode() {
                crate::SHUTDOWN.store(true, Ordering::Relaxed);
            }
        }
    }
}

async fn transmit(mut tx: CanTx<'static>) {
    let mut mailbox = None;
    loop {
        let battery_voltage_mv = BATTERY_VOLTAGE_MV.load(Ordering::Relaxed);
        let output_current_ma = OUTPUT_CURRENT_MA.load(Ordering::Relaxed);
        let output_voltage_mv = OUTPUT_VOLTAGE_MV.load(Ordering::Relaxed);

        let data = BatteryData {
            battery_voltage_mv,
            output_voltage_mv,
            output_current_ma,
        };

        if let Some(frame) = data.try_encode() {
            if let Some(mbox) = mailbox.take() {
                let r = tx.abort(mbox);
                info!("CAN sent: {}", r);
            }
            if let Ok(wr) = tx.try_write(&frame) {
                mailbox = Some(wr.mailbox());
            }
        }

        Timer::after_millis(100).await;
    }
}

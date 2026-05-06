#![no_std]

use num_enum::{TryFromPrimitive, IntoPrimitive};

pub use can_messages_trait::prelude::*;

pub mod prelude {
    pub use can_messages_trait::prelude::*;
}

pub const BITRATE: u32 = 1_000_000;

#[repr(u16)]
#[derive(Debug, TryFromPrimitive, IntoPrimitive, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CanId {
    POWEROFF = 0b_000_0000_0001,
    BATTERY = 0b_001_0001_0001,
    COOLBOX = 0b_001_0010_0001,
}

#[can_message(CanId::POWEROFF)]
pub struct PowerOff;

#[can_message(CanId::BATTERY)]
pub struct BatteryData {
    pub battery_voltage_mv: u16,
    pub output_voltage_mv: i16,
    pub output_current_ma: i16,
}

#[can_message(CanId::COOLBOX)]
pub struct CoolBox {
    pub box_temperature_deg10: i16,
    pub pwm_duty: u8,
    pub _reserved: u8,
}

/*
can_variant!{BatterySignals {
    Pow(PowerOff),
    Bat(BatteryData),
}}*/

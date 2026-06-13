use core::convert::Infallible;
use embassy_time::Instant;
use embedded_hal::{
    digital::OutputPin,
    pwm::{ErrorType as PwmErrorType, SetDutyCycle},
};
use num_traits::float::FloatCore;
use unwrap_infallible::UnwrapInfallible;
use defmt::info;

use crate::traits::Output;

pub struct Pwm<P, E> {
    pwm: P,
    en_pin: E,
    enabled: bool,
    last_change: Instant,
    max_rate: f32,
    current_duty: f32,
}

impl<P, E> Pwm<P, E>
where
    P: PwmErrorType<Error = Infallible> + SetDutyCycle,
    E: OutputPin<Error = Infallible>,
{
    pub fn new(pwm: P, en_pin: E, max_rate: f32) -> Self {
        let last_change = Instant::now();
        Pwm {
            pwm,
            en_pin,
            enabled: false,
            last_change,
            max_rate,
            current_duty: 0.0,
        }
    }
}

impl<P, E> Output for Pwm<P, E>
where
    P: PwmErrorType<Error = Infallible> + SetDutyCycle,
    E: OutputPin<Error = Infallible>,
{
    type Value = f32;

    fn current_value(&self) -> Self::Value {
        self.current_duty
    }

    fn set_output(&mut self, at: Instant, value: Self::Value) {
        let dt = (at - self.last_change).as_micros() as f32 / 1_000_000.0;
        self.last_change = at;
        let rate = (self.max_rate * dt).clamp(0.0, 0.001);
        self.current_duty =
            (self.current_duty + (value - self.current_duty).clamp(-rate, rate)).clamp(0.0, 1.0);
        let enable = self.current_duty > f32::EPSILON;
        //info!("PWM duty = {}", self.current_duty);
        let duty = (self.current_duty * u16::MAX as f32)
            .round()
            .clamp(0.0, u16::MAX as f32) as u16;
        self.pwm
            .set_duty_cycle_fraction(duty, u16::MAX)
            .unwrap_infallible();
        if enable != self.enabled {
            self.enabled = enable;
            self.en_pin.set_state(enable.into());
        }
    }
}


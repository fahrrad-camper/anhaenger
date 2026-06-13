use super::traits::{AsInput, Regulator};
use core::convert::Infallible;
use embedded_hal::{
    digital::OutputPin,
    pwm::{ErrorType as PwmErrorType, SetDutyCycle},
};
use libm::sqrtf;
use embassy_time::Instant;

const MIN_POWER: f32 = 0.001;
const DEFAULT_DUTY_PER_WATT: f32 = 0.01;

pub struct PowerRegulator<P> {
    pwm: P,
    k: f32,
}

impl<P> PowerRegulator<P> {
    pub fn new(pwm: P) -> Self {
        Self {
            pwm,
            k: DEFAULT_DUTY_PER_WATT,
        }
    }
}

impl<P, I> Regulator<I> for PowerRegulator<P>
where
    I: AsInput<Self, Value = f32>,
    P: Regulator<I, Value = I::Value>,
{
    type Value = f32;

    fn current_value(&self, input: &I) -> Self::Value {
        input.as_input()
    }

    fn regulate(&mut self, at: Instant, input: &I, target: Self::Value) {
        let current_power = input.as_input();
        let current_duty = self.pwm.current_value(input);
        if current_power > MIN_POWER {
            self.k = (current_duty * current_duty) / current_power;
        }
        let desired_duty = sqrtf(self.k * target);
        defmt::info!("duty = {}", desired_duty);
        self.pwm.regulate(at, input, desired_duty);
    }
}


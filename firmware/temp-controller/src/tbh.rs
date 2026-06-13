use core::mem::replace;
use embassy_time::Instant;

use crate::traits::{AsInput, Regulator};

pub struct Tbh<T> {
    k: f32,
    output: T,
    out_factor: f32,
    last_feed: Instant,
    last_positive: bool,
    value_at_last_crossing: Option<f32>,
}

impl<T> Tbh<T> {
    pub fn new(k: f32, output: T, out_factor: f32) -> Self {
        let last_feed = Instant::now();
        Self {
            output,
            out_factor,
            last_feed,
            k,
            last_positive: false,
            value_at_last_crossing: None,
        }
    }
}

impl<T, I> Regulator<I> for Tbh<T>
where
    I: AsInput<Self, Value = f32>,
    T: Regulator<I, Value = I::Value>,
{
    type Value = f32;

    fn current_value(&self, input: &I) -> Self::Value {
        input.as_input()
    }

    fn regulate(&mut self, at: Instant, input: &I, target: Self::Value) {
        let current = input.as_input();
        let delta = current - target;
        let dt = (at - self.last_feed).as_micros() as f32 / 1_000_000.0;
        let mut value = self.output.current_value(input);
        value = (value - self.k * dt * delta).clamp(0.0, 1.0);
        let positive = delta.signum() > 0.0;
        if self.value_at_last_crossing.is_none() {
            self.value_at_last_crossing = Some(value);
            self.last_positive = positive;
        } else {
            if positive != self.last_positive {
                let v = replace(&mut self.value_at_last_crossing, Some(value)).unwrap();
                self.last_positive = positive;
                value = (value + v) / 2.0;
            }
        }
        self.output.regulate(at, input, value * self.out_factor);
    }
}

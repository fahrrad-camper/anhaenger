use embassy_time::Instant;

pub trait AsInput<T> {
    type Value;
    fn as_input(&self) -> Self::Value;
}

pub trait Regulator<I> {
    type Value;
    fn current_value(&self, input: &I) -> Self::Value;
    fn regulate(&mut self, at: Instant, input: &I, target: Self::Value);
}

pub trait Output {
    type Value;
    fn current_value(&self) -> Self::Value;
    fn set_output(&mut self, at: Instant, value: Self::Value);
}

impl<O, I> Regulator<I> for O
where
    O: Output,
{
    type Value = O::Value;
    fn current_value(&self, _input: &I) -> Self::Value {
        self.current_value()
    }
    fn regulate(&mut self, at: Instant, _input: &I, value: Self::Value) {
        self.set_output(at, value)
    }
}

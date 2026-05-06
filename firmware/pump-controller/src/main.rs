#![feature(impl_trait_in_assoc_type)]
#![no_std]
#![no_main]

const A: u16 = 115 * 1;
const B: u16 = 097 * 3;

use {defmt_rtt as _, panic_probe as _};

use defmt::info;
use embassy_executor::{main, Spawner};
use embassy_futures::select::{select, Either};
use embassy_stm32::{
    bind_interrupts,
    exti::{self, ExtiInput},
    gpio::{Input, Level, OutputOpenDrain, OutputType, Pull, Speed},
    interrupt, pac,
    time::khz,
    timer::{
        complementary_pwm::{ComplementaryPwm, ComplementaryPwmPin},
        low_level::CountingMode,
        Channel,
    },
    Config as DeviceConfig,
};
use embassy_time::Timer;

bind_interrupts!(struct Irqs {
    EXTI4_15 => exti::InterruptHandler<interrupt::typelevel::EXTI4_15>;
});

#[main]
async fn main(_spawner: Spawner) {
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

    info!("System startup");

    let mut dir1 = OutputOpenDrain::new(dev.PA4, Level::Low, Speed::Low);
    let mut dir2 = OutputOpenDrain::new(dev.PA3, Level::Low, Speed::Low);

    let motor1 = ComplementaryPwmPin::new(dev.PA7, OutputType::OpenDrain);
    let motor2 = ComplementaryPwmPin::new(dev.PB1, OutputType::OpenDrain);
    let mut pwmgen = ComplementaryPwm::new(
        dev.TIM1,
        None,
        Some(motor1),
        None,
        None,
        None,
        Some(motor2),
        None,
        None,
        khz(10),
        CountingMode::EdgeAlignedUp,
    );

    pwmgen.set_duty(Channel::Ch1, 0);
    pwmgen.set_duty(Channel::Ch3, 0);
    pwmgen.enable(Channel::Ch1);
    pwmgen.enable(Channel::Ch3);

    let in1 = Input::new(dev.PA6, Pull::Up);
    let in2 = Input::new(dev.PA5, Pull::Up);

    let mut switch = ExtiInput::new(dev.PB8, dev.EXTI8, Pull::Up, Irqs);

    const S: u16 = 50;

    let max = pwmgen.get_max_duty();

    let mut delta = Delta::new(in1, A, in2, B);

    loop {
        // Wait for button press
        loop {
            switch.wait_for_low().await;
            let s = select(Timer::after_millis(20), switch.wait_for_high()).await;
            if let Either::First(()) = s {
                break;
            }
        }

        delta.reset();

        info!("Doing dosage {}:{}", A, B);
        // Main dosage loop.
        while switch.is_low() {
            delta.count();
            // Compute desired motor ratio.
            let v1 = S as i32 * B as i32;
            let v2 = S as i32 * A as i32 + delta.get();
            const MAX: i32 = 32768;
            const SPD: i32 = MAX / 2;
            let (m1, m2) = if v1 > v2 {
                (SPD, SPD * v2 / v1)
            } else {
                (SPD * v1 / v2, SPD)
            };
            // Set motor PWM values.
            pwmgen.set_duty(Channel::Ch1, (m1 * max as i32 / MAX).try_into().unwrap());
            pwmgen.set_duty(Channel::Ch3, (m2 * max as i32 / MAX).try_into().unwrap());
        }

        let old_delta = delta.get();

        // Compensate for last droplets.
        if delta.get() > 0 {
            pwmgen.set_duty(Channel::Ch1, 0);
            pwmgen.set_duty(Channel::Ch3, max);
            while delta.get() > 0 {
                delta.count()
            }
        } else if delta.get() < 0 {
            pwmgen.set_duty(Channel::Ch1, max);
            pwmgen.set_duty(Channel::Ch3, 0);
            while delta.get() < 0 {
                delta.count()
            }
        }
        dir1.set_high();
        dir2.set_high();
        pwmgen.set_duty(Channel::Ch1, max);
        pwmgen.set_duty(Channel::Ch3, max);
        Timer::after_millis(1).await;
        pwmgen.set_duty(Channel::Ch1, 0);
        pwmgen.set_duty(Channel::Ch3, 0);
        dir1.set_low();
        dir2.set_low();

        info!("Done; compensated {} and got {}", old_delta, delta.get());
    }
}

struct Delta<'t> {
    ia: Input<'t>,
    ib: Input<'t>,
    oa: bool,
    ob: bool,
    a: u16,
    b: u16,
    value: i32,
}

impl<'t> Delta<'t> {
    fn new(ia: Input<'t>, a: u16, ib: Input<'t>, b: u16) -> Self {
        let value = 0;
        let oa = ia.is_low();
        let ob = ib.is_low();
        Self {
            ia,
            ib,
            oa,
            ob,
            a,
            b,
            value,
        }
    }

    fn get(&self) -> i32 {
        self.value
    }

    fn reset(&mut self) {
        self.value = 0
    }

    fn count(&mut self) {
        let va = self.ia.is_low();
        let vb = self.ib.is_low();
        if va != self.oa {
            self.value += self.a as i32;
        }
        if vb != self.ob {
            self.value -= self.b as i32;
        }
        self.oa = va;
        self.ob = vb;
    }
}

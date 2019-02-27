//! Augmented STM32 operations.
//!
//! This is a set of extensions and workarounds for the `stm32f4` crate.

use stm32f4::stm32f407 as device;

/// A representation of the clock config parameters for the STM32F4 RCC when
/// using the High Speed External option with the PLL.
pub struct ClockConfig {
    pub crystal_hz: f32,
    pub crystal_divisor: u8,
    pub vco_multiplier: u16,
    pub general_divisor: device::rcc::pllcfgr::PLLPW,
    pub pll48_divisor: u8,

    pub ahb_divisor: device::rcc::cfgr::HPREW,
    pub apb1_divisor: device::rcc::cfgr::PPRE1W,
    pub apb2_divisor: device::rcc::cfgr::PPRE2W,

    pub flash_latency: device::flash::acr::LATENCYW,
}

/// I have to do this manually because svd2rust thinks enums have identity.
impl Clone for ClockConfig {
    fn clone(&self) -> Self {
        ClockConfig {
            general_divisor: self.general_divisor.copy_hack(),
            ahb_divisor: self.ahb_divisor.copy_hack(),
            apb1_divisor: self.apb1_divisor.copy_hack(),
            apb2_divisor: self.apb2_divisor.copy_hack(),
            flash_latency: self.flash_latency.copy_hack(),
            ..*self
        }
    }
}

macro_rules! block_while {
    ($condition:expr) => {
        while $condition {}
    };
}

macro_rules! block_until {
    ($condition:expr) => {
        block_while!(!$condition)
    };
}

/// Applies the settings described in `cfg` to the `rcc` and `flash`. (The flash
/// controller gets involved because we have to adjust wait states.)
///
/// The algorithm used can transition from any valid clock config to any other,
/// by switching to the internal high-speed oscillator in between modes.
pub fn configure_clocks(
    rcc: &device::RCC,
    flash: &device::FLASH,
    cfg: &ClockConfig,
) {
    // Switch to the internal 16MHz oscillator while messing with the PLL.
    rcc.cr.modify(|_, w| w.hsion().set_bit());
    // Wait for it to stabilize.
    block_until! { rcc.cr.read().hsirdy().bit() }
    // Make the switch.
    rcc.cfgr
        .modify(|_, w| w.sw().variant(device::rcc::cfgr::SWW::HSI));
    // Wait for it.
    block_until! { rcc.cfgr.read().sws() == device::rcc::cfgr::SWSR::HSI }

    // Turn off the PLL.
    rcc.cr.modify(|_, w| w.pllon().clear_bit());
    block_while! { rcc.cr.read().pllrdy().bit() }

    // Apply divisors before boosting frequency.
    rcc.cfgr.modify(|_, w| {
        w.hpre()
            .variant(cfg.ahb_divisor.copy_hack())
            .ppre1()
            .variant(cfg.apb1_divisor.copy_hack())
            .ppre2()
            .variant(cfg.apb2_divisor.copy_hack())
    });

    flash
        .acr
        .modify(|_, w| w.latency().variant(cfg.flash_latency.copy_hack()));

    // Switch on the crystal oscillator.
    rcc.cr.modify(|_, w| w.hseon().set_bit());
    block_until! { rcc.cr.read().hserdy().bit() }

    // Configure the PLL.

    rcc.pllcfgr.modify(|_, w| {
        // Safety: only unsafe due to upstream bug. TODO
        unsafe {
            w.pllm().bits(cfg.crystal_divisor);
        }
        // Safety: only unsafe due to upstream bug. TODO
        unsafe {
            w.plln().bits(cfg.vco_multiplier);
        }
        // Safety: only unsafe due to upstream bug. TODO
        unsafe {
            w.pllq().bits(cfg.pll48_divisor);
        }
        w.pllp()
            .variant(cfg.general_divisor.copy_hack()) // half yay/half TODO
            .pllsrc()
            .variant(device::rcc::pllcfgr::PLLSRCW::HSE) // yay
    });

    // Turn it on.
    rcc.cr.modify(|_, w| w.pllon().set_bit());
    block_until! { rcc.cr.read().pllrdy().bit() }

    // Select PLL as clock source.
    rcc.cfgr
        .modify(|_, w| w.sw().variant(device::rcc::cfgr::SWW::PLL));
    block_until! { rcc.cfgr.read().sws() == device::rcc::cfgr::SWSR::PLL }
}

/// Weld some operations onto svd2rust divisor enums to make them very slightly
/// more useful. Still not Copy for some reason.
pub trait UsefulDivisor {
    fn divisor(&self) -> usize;
}

/// Slap a copy operation onto types that aren't Copy for some reason.
///
/// This trait is `unsafe` because you had better know what you're doing if you
/// implement it for a foreign type.
pub unsafe trait CopyHack: Sized {
    fn copy_hack(&self) -> Self {
        unsafe {
            // wheeeee
            core::ptr::read(self)
        }
    }
}

impl UsefulDivisor for device::rcc::cfgr::HPREW {
    fn divisor(&self) -> usize {
        use device::rcc::cfgr::HPREW;
        match self {
            HPREW::DIV1 => 1,
            HPREW::DIV2 => 2,
            HPREW::DIV4 => 4,
            HPREW::DIV8 => 8,
            HPREW::DIV16 => 16,
            HPREW::DIV64 => 64,
            HPREW::DIV128 => 128,
            HPREW::DIV256 => 256,
            HPREW::DIV512 => 512,
        }
    }
}

unsafe impl CopyHack for device::rcc::cfgr::HPREW {}

impl UsefulDivisor for device::rcc::cfgr::PPRE2W {
    fn divisor(&self) -> usize {
        use device::rcc::cfgr::PPRE2W;
        match self {
            PPRE2W::DIV1 => 1,
            PPRE2W::DIV2 => 2,
            PPRE2W::DIV4 => 4,
            PPRE2W::DIV8 => 8,
            PPRE2W::DIV16 => 16,
        }
    }
}

unsafe impl CopyHack for device::rcc::cfgr::PPRE2W {}
unsafe impl CopyHack for device::flash::acr::LATENCYW {}
unsafe impl CopyHack for device::rcc::pllcfgr::PLLPW {}
unsafe impl CopyHack for device::Interrupt {}
unsafe impl CopyHack for device::dma2::s0cr::W {}
unsafe impl CopyHack for device::dma2::s1cr::W {}
unsafe impl CopyHack for device::dma2::s2cr::W {}
unsafe impl CopyHack for device::dma2::s3cr::W {}
unsafe impl CopyHack for device::dma2::s4cr::W {}
unsafe impl CopyHack for device::dma2::s5cr::W {}
unsafe impl CopyHack for device::dma2::s6cr::W {}
unsafe impl CopyHack for device::dma2::s7cr::W {}

/// Trait for welding variant support onto an un-modeled register field.
pub trait VariantExt<V> {
    type W;
    fn variant(self, variant: V) -> Self::W;
}

/// Trait for welding arbitrary write support onto an un-modeled register field.
pub trait AllWriteExt<T> {
    type W;
    fn bits_ext(self, value: T) -> Self::W;
}

pub mod tim1 {
    pub mod arr {
        use stm32f4::stm32f407::tim1::arr as device;

        impl<'a> crate::util::stm32::AllWriteExt<u16> for device::_ARRW<'a> {
            type W = &'a mut device::W;
            fn bits_ext(self, value: u16) -> Self::W {
                unsafe { self.bits(value) }
            }
        }
    }
    pub mod cnt {
        use stm32f4::stm32f407::tim1::cnt as device;

        impl<'a> crate::util::stm32::AllWriteExt<u16> for device::_CNTW<'a> {
            type W = &'a mut device::W;
            fn bits_ext(self, value: u16) -> Self::W {
                unsafe { self.bits(value) }
            }
        }
    }
}

pub mod tim3 {
    pub mod smcr {
        use stm32f4::stm32f407::tim3::smcr as device;

        #[allow(non_camel_case_types)]
        #[derive(Copy, Clone, Debug)]
        pub enum TSW {
            // Internal Trigger 0
            ITR0 = 0b000,
            // Internal Trigger 1
            ITR1 = 0b001,
            // Internal Trigger 2
            ITR2 = 0b010,
            // Internal Trigger 3
            ITR3 = 0b011,
            // TI1 Edge Detector
            TI1F_ED = 0b100,
            // Filtered Timer Input 1
            TI1FP1 = 0b101,
            // Filtered Timer Input 2
            TI2FP2 = 0b110,
            // External Trigger input
            ETRF = 0b111,
        }

        impl<'a> crate::util::stm32::VariantExt<TSW> for device::_TSW<'a> {
            type W = &'a mut device::W;
            fn variant(self, variant: TSW) -> Self::W {
                unsafe { self.bits(variant as u8) }
            }
        }

        #[derive(Copy, Clone, Debug)]
        pub enum SMSW {
            // 000: Slave mode disabled - if CEN = ‘1 then the prescaler is
            //      clocked directly by the internal clock.
            Disabled = 0b000,
            // 001: Encoder mode 1 - Counter counts up/down on TI2FP1 edge
            //      depending on TI1FP2 level.
            Encoder1 = 0b001,
            // 010: Encoder mode 2 - Counter counts up/down on TI1FP2 edge
            //      depending on TI2FP1 level.
            Encoder2 = 0b010,
            // 011: Encoder mode 3 - Counter counts up/down on both TI1FP1 and
            //      TI2FP2 edges depending on the level of the other input.
            Encoder3 = 0b011,
            // 100: Reset Mode - Rising edge of the selected trigger input
            //      (TRGI) reinitializes the counter and generates an update of
            //      the registers.
            Reset = 0b100,
            // 101: Gated Mode - The counter clock is enabled when the trigger
            //      input (TRGI) is high. The counter stops (but is not reset)
            //      as soon as the trigger becomes low. Both start and stop of
            //      the counter are controlled.
            Gated = 0b101,
            // 110: Trigger Mode - The counter starts at a rising edge of the
            //      trigger TRGI (but it is not reset). Only the start of the
            //      counter is controlled.
            Trigger = 0b110,
            // 111: External Clock Mode 1 - Rising edges of the selected trigger
            //      (TRGI) clock the counter.
            External = 0b111,
        }

        impl<'a> crate::util::stm32::VariantExt<SMSW> for device::_SMSW<'a> {
            type W = &'a mut device::W;
            fn variant(self, variant: SMSW) -> Self::W {
                unsafe { self.bits(variant as u8) }
            }
        }
    }
    pub mod psc {
        use stm32f4::stm32f407::tim3::psc as device;

        impl<'a> crate::util::stm32::AllWriteExt<u16> for device::_PSCW<'a> {
            type W = &'a mut device::W;
            fn bits_ext(self, value: u16) -> Self::W {
                unsafe { self.bits(value) }
            }
        }
    }
    pub mod ccmr1_output {
        use stm32f4::stm32f407::tim3::ccmr1_output as device;

        #[derive(Copy, Clone, Debug)]
        pub enum OC1MW {
            // 000: Frozen - The comparison between the output compare register
            //      TIMx_CCR1 and the counter TIMx_CNT has no effect on the
            //      outputs.(this mode is used to generate a timing base).
            Frozen = 0b000,
            // 001: Set channel 1 to active level on match. OC1REF signal is
            //      forced high when the counter TIMx_CNT matches the
            //      capture/compare register 1 (TIMx_CCR1).
            Set1Active = 0b001,
            // 010: Set channel 1 to inactive level on match. OC1REF signal is
            //      forced low when the counter TIMx_CNT matches the
            //      capture/compare register 1 (TIMx_CCR1).
            Set1Inactive = 0b010,
            // 011: Toggle - OC1REF toggles when TIMx_CNT=TIMx_CCR1.
            Toggle = 0b011,
            // 100: Force inactive level - OC1REF is forced low.
            Inactive = 0b100,
            // 101: Force active level - OC1REF is forced high.
            Active = 0b101,
            // 110: PWM mode 1 - In upcounting, channel 1 is active as long as
            //      TIMx_CNT<TIMx_CCR1 else inactive. In downcounting, channel 1
            //      is inactive (OC1REF=‘0) as long as TIMx_CNT>TIMx_CCR1 else
            //      active (OC1REF=1).
            Pwm1 = 0b110,
            // 111: PWM mode 2 - In upcounting, channel 1 is inactive as long as
            //      TIMx_CNT<TIMx_CCR1 else active. In downcounting, channel 1
            //      is active as long as TIMx_CNT>TIMx_CCR1 else inactive.
            Pwm2 = 0b111,
        }

        impl<'a> crate::util::stm32::VariantExt<OC1MW> for device::_OC1MW<'a> {
            type W = &'a mut device::W;
            fn variant(self, variant: OC1MW) -> Self::W {
                unsafe { self.bits(variant as u8) }
            }
        }

        #[derive(Copy, Clone, Debug)]
        pub enum CC1SW {
            // 00: CC1 channel is configured as output.
            Output = 0b00,
            // 01: CC1 channel is configured as input, IC1 is mapped on TI1.
            InputTi1 = 0b01,
            // 10: CC1 channel is configured as input, IC1 is mapped on TI2.
            InputTi2 = 0b10,
            // 11: CC1 channel is configured as input, IC1 is mapped on TRC.
            //     This mode is working only if an internal trigger input is
            //     selected through TS bit (TIMx_SMCR register)
            InputTrc = 0b11,
        }

        impl<'a> crate::util::stm32::VariantExt<CC1SW> for device::_CC1SW<'a> {
            type W = &'a mut device::W;
            fn variant(self, variant: CC1SW) -> Self::W {
                unsafe { self.bits(variant as u8) }
            }
        }

    }
}

pub mod gpiob {
    pub mod bsrr {
        use stm32f4::stm32f407::gpiob::bsrr as device;
        impl<'a> crate::util::stm32::AllWriteExt<u32> for &'a mut device::W {
            type W = &'a mut device::W;
            fn bits_ext(self, value: u32) -> Self::W {
                unsafe { self.bits(value) }
            }
        }
    }
}

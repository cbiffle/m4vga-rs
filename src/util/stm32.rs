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
    ($condition:expr) => { block_while!(!$condition) };
}

/// Applies the settings described in `cfg` to the `rcc` and `flash`. (The flash
/// controller gets involved because we have to adjust wait states.)
///
/// The algorithm used can transition from any valid clock config to any other,
/// by switching to the internal high-speed oscillator in between modes.
pub fn configure_clocks(rcc: &device::RCC,
                        flash: &device::FLASH,
                        cfg: &ClockConfig) {
    // Switch to the internal 16MHz oscillator while messing with the PLL.
    rcc.cr.modify(|_, w| w.hsion().set_bit());
    // Wait for it to stabilize.
    block_until! { rcc.cr.read().hsirdy().bit() }
    // Make the switch.
    rcc.cfgr.modify(|_, w| w.sw().variant(device::rcc::cfgr::SWW::HSI));
    // Wait for it.
    block_until! { rcc.cfgr.read().sws() == device::rcc::cfgr::SWSR::HSI }

    // Turn off the PLL.
    rcc.cr.modify(|_, w| w.pllon().clear_bit());
    block_while! { rcc.cr.read().pllrdy().bit() }

    // Apply divisors before boosting frequency.
    rcc.cfgr.modify(|_, w| w
                    .hpre().variant(cfg.ahb_divisor.copy_hack())
                    .ppre1().variant(cfg.apb1_divisor.copy_hack())
                    .ppre2().variant(cfg.apb2_divisor.copy_hack()));

    flash.acr.modify(|_, w| w.latency().variant(cfg.flash_latency.copy_hack()));

    // Switch on the crystal oscillator.
    rcc.cr.modify(|_, w| w.hseon().set_bit());
    block_until! { rcc.cr.read().hserdy().bit() }

    // Configure the PLL.

    rcc.pllcfgr.modify(|_, w| {
        unsafe { w.pllm().bits(cfg.crystal_divisor); } // TODO
        unsafe { w.plln().bits(cfg.vco_multiplier); } // TODO
        unsafe { w.pllq().bits(cfg.pll48_divisor); } // TODO
        w.pllp().variant(cfg.general_divisor.copy_hack()) // half yay/half TODO
            .pllsrc().variant(device::rcc::pllcfgr::PLLSRCW::HSE) // yay
    });

    // Turn it on.
    rcc.cr.modify(|_, w| w.pllon().set_bit());
    block_until! { rcc.cr.read().pllrdy().bit() }

    // Select PLL as clock source.
    rcc.cfgr.modify(|_, w| w.sw().variant(device::rcc::cfgr::SWW::PLL));
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

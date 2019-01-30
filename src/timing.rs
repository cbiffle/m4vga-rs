use crate::util::stm32;
use stm32f4::stm32f407 as device;

pub const MIN_CYCLES_PER_PIXEL: usize = 4;

// TODO: I want this to be Debug, but svd2rust hates me.
/// Defines the timing parameters for a video mode.
#[derive(Clone)]
pub struct Timing {
    pub clock_config: stm32::ClockConfig,

    /// Number of additional AHB cycles per pixel clock cycle. The driver uses a
    /// minimum of 4 cycles per pixel; this field adds to that. Larger values
    /// reduce both the resolution and the compute/bandwidth requirements.
    pub add_cycles_per_pixel: usize,

    /// Total horizontal pixels per line, including blanking.
    pub line_pixels: usize,
    /// Length of horizontal sync pulse.
    pub sync_pixels: usize,
    /// Number of pixels between end of sync and start of video.
    pub back_porch_pixels: usize,
    /// Fudge factor, nudging the DMA interrupt backwards in time to compensate
    /// for startup code taking non-zero time.
    pub video_lead: usize,
    /// Maximum visible pixels per line.
    pub video_pixels: usize,
    /// Polarity of horizontal sync pulse.
    pub hsync_polarity: Polarity,

    /// Scanline number of onset of vertical sync pulse, numbered from end of
    /// active video.
    pub vsync_start_line: usize,
    /// Scanline number of end of vertical sync pulse, numbered from end of
    /// active video.
    pub vsync_end_line: usize,
    /// Scanline number of start of active video, numbered from end of active
    /// video.
    pub video_start_line: usize,
    /// Scanline number of end of active video, numbered from end of active
    /// video in the last frame. This is the number of lines per frame.
    pub video_end_line: usize,
    /// Polarity of the vertical sync pulse.
    pub vsync_polarity: Polarity,
}

impl Timing {
    /// Compute total AHB cycles per pixel in this timing mode.
    pub fn cycles_per_pixel(&self) -> usize {
        self.add_cycles_per_pixel + MIN_CYCLES_PER_PIXEL
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Polarity {
    Positive = 0,
    Negative = 1,
}


/// Industry standard 800x600 60Hz timing.
///
/// This produces a 160MHz CPU clock speed for a 40MHz pixel clock.
pub static SVGA_800_600: Timing = Timing {
    clock_config: stm32::ClockConfig {
        crystal_hz: 8000000.0,// external crystal Hz
        crystal_divisor: 4,   // divide down to 2Mhz
        vco_multiplier: 160,  // multiply up to 320MHz VCO
        // divide by 2 for 160MHz CPU clock
        general_divisor: device::rcc::pllcfgr::PLLPW::DIV2,
        pll48_divisor: 7,     // divide by 7 for 48MHz-ish SDIO clock
        // divide CPU clock by 1 for 160MHz AHB clock
        ahb_divisor: device::rcc::cfgr::HPREW::DIV1,
        // divide CPU clock by 4 for 40MHz APB1 clock.
        apb1_divisor: device::rcc::cfgr::PPRE2W::DIV4,
        // divide CPU clock by 2 for 80MHz APB2 clock.
        apb2_divisor: device::rcc::cfgr::PPRE2W::DIV2,

        // 5 wait states for 160MHz at 3.3V.
        flash_latency: device::flash::acr::LATENCYW::WS5,
    },

    add_cycles_per_pixel: 0,

    line_pixels      : 1056,
    sync_pixels      : 128,
    back_porch_pixels: 88,
    video_lead       : 20,
    video_pixels     : 800,
    hsync_polarity   : Polarity::Positive,

    vsync_start_line: 1,
    vsync_end_line  : 1 + 4,
    video_start_line: 1 + 4 + 23,
    video_end_line  : 1 + 4 + 23 + 600,
    vsync_polarity  : Polarity::Positive,
};

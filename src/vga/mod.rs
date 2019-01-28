pub mod rast;

pub mod hstate;
pub mod bg_rast;
pub mod shock;

use stm32f4::stm32f407 as device;
use cortex_m::peripheral as cm;

use cortex_m::peripheral::scb::SystemHandler;

use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use crate::armv7m::*;
use crate::stm32::{ClockConfig, copy_interrupt, UsefulDivisor, configure_clocks};
use crate::util::spin_lock::{SpinLock, SpinLockGuard};

use self::rast::{RasterCtx, TargetBuffer};

pub type Pixel = u8;

pub const MAX_PIXELS_PER_LINE: usize = 800;

const SHOCK_ABSORBER_SHIFT_CYCLES: u32 = 20;

struct HStateHw {
    dma2: device::DMA2,     // PendSV HState
    tim1: device::TIM1,     // PendSV HState
    tim4: device::TIM4,     //        HState
    gpiob: device::GPIOB,   //        HState
}

static HSTATE_HW: SpinLock<Option<HStateHw>> = SpinLock::new(None);

/// Records when a driver instance has been initialized. This is only allowed to
/// happen once at the moment because we don't have perfect teardown code.
static DRIVER_INIT_FLAG: AtomicBool = AtomicBool::new(false);

/// Equivalent of `rast::TargetBuffer`, but as words to ensure alignment for
/// certain of our high-throughput routines.
type WorkingBuffer = [u32; WORKING_BUFFER_SIZE];
const WORKING_BUFFER_SIZE: usize = rast::TARGET_BUFFER_SIZE / 4;

/// Groups parameters passed from PendSV to HState ISR, describing the next DMA
/// transfer.
struct NextTransfer {
    /// Bitwise representation of the DMA SxCR register value that starts the
    /// transfer.
    ///
    /// TODO: this should probably be `device::dma2::s5cr::W`, but that type has
    /// no `const` constructor for use in a static, and is generally just a pain
    /// in the ass to work with, so `u32` it is.
    dma_cr_bits: u32,
    /// Whether to use timer-mediated DMA to decrease horizontal resolution.
    use_timer: bool,
}

/// Parameters passed from PendSV to HState ISR.
static NEXT_XFER: SpinLock<NextTransfer> = SpinLock::new(NextTransfer {
    dma_cr_bits: 0,
    use_timer: false,
});

// TODO: I want this to be Debug, but svd2rust hates me.
#[derive(Clone)]
pub struct Timing {
    pub clock_config: ClockConfig,

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

const MIN_CYCLES_PER_PIXEL: usize = 4;

impl Timing {
    pub fn cycles_per_pixel(&self) -> usize {
        self.add_cycles_per_pixel + MIN_CYCLES_PER_PIXEL
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Polarity {
    Positive = 0,
    Negative = 1,
}

/// Driver state.
///
/// The `MODE` parameter tracks the state of the driver at compile time, and
/// changes what operations are available.
pub struct Vga<MODE> {
    rcc: device::RCC,
    flash: device::FLASH,
    gpioe: device::GPIOE,
    tim3: device::TIM3,
    nvic: cm::NVIC,  // TODO probably should not own this

    mode_state: MODE,
}

/// Driver mode right after initialization, but before `configure_timing`.
pub struct Idle(HStateHw);

/// Driver mode once timing has been configured.
pub struct Sync(());

/// Driver mode once rasterization has been configured.
pub struct Live(());

pub trait SyncOn {}

impl SyncOn for Sync {}
impl SyncOn for Live {}

static RASTER: rast::IRef = rast::IRef::new();

/// Operations valid in any driver state.
impl<T> Vga<T> {
    /// Disables video output. This is not synchronized and can happen in the
    /// middle of the frame; if that bothers you, synchronize with vblank.
    pub fn video_off(&self) {
        self.gpioe.pupdr.modify(|_, w| w
                                .pupdr8().pull_down()
                                .pupdr9().pull_down()
                                .pupdr10().pull_down()
                                .pupdr11().pull_down()
                                .pupdr12().pull_down()
                                .pupdr13().pull_down()
                                .pupdr14().pull_down()
                                .pupdr15().pull_down());
        self.gpioe.moder.modify(|_, w| w
                                .moder8().input()
                                .moder9().input()
                                .moder10().input()
                                .moder11().input()
                                .moder12().input()
                                .moder13().input()
                                .moder14().input()
                                .moder15().input());
    }
}

/// Operations valid in any driver state where sync is being generated.
impl<T: SyncOn> Vga<T> {
    /// Busy-waits for the transition from active video to vertical blank.
    /// Because this waits for the *transition*, if you call this *during*
    /// vblank it will wait for an entire frame.
    pub fn sync_to_vblank(&self) {
        unimplemented!()
    }
}

fn sync_off(gpiob: &device::GPIOB) {
    gpiob.moder.modify(|_, w| w
                       .moder6().input()
                       .moder7().input());
    gpiob.pupdr.modify(|_, w| w
                       .pupdr6().pull_down()
                       .pupdr7().pull_down());
}

fn sync_on(gpiob: &device::GPIOB) {
    // Configure PB6/PB7 for fairly sharp edges.
    gpiob.ospeedr.modify(|_, w| w
                         .ospeedr6().high_speed()
                         .ospeedr7().high_speed());
    // Disable pullups/pulldowns.
    gpiob.pupdr.modify(|_, w| w
                       .pupdr6().floating()
                       .pupdr7().floating());
    // Configure PB6 as AF2 and PB7 as output.
    gpiob.afrl.modify(|_, w| w.afrl6().af2());
    gpiob.moder.modify(|_, w| w
                       .moder6().alternate()
                       .moder7().output());
}

/// Operations that are only valid before timing has been configured.
impl Vga<Idle> {
    /// Configures video timing.
    ///
    /// TODO: currently there is no way to re-configure timing, but the C++
    /// implementation supports that feature. The absence is a side effect of
    /// the type-state encoding of Vga. Provide a replacement.
    pub fn configure_timing(mut self, timing: &Timing) -> Vga<Sync> {
        // TODO: timing consistency asserts

        //TODO re-enable when reconfiguration happens
        //self.video_off(); // TODO: move into with_raster
        //self.sync_off();

        // Place the horizontal timers in reset, disabling interrupts.
        disable_h_timer(
            &mut self.nvic,
            &device::Interrupt::TIM4, 
            &self.rcc,
            |w| w.tim4rst().set_bit(),
        );
        disable_h_timer(
            &mut self.nvic,
            &device::Interrupt::TIM3,
            &self.rcc,
            |w| w.tim3rst().set_bit(),
        );

        // Busy-wait for pending DMA to complete.
        while self.mode_state.0.dma2.s5cr.read().en().bit_is_set() {
            // busy wait
        }

        // Switch to new CPU clock settings.
        configure_clocks(&self.rcc, &self.flash, &timing.clock_config);

        // Configure TIM3/4 for horizontal sync generation.
        configure_h_timer(
            timing,
            &self.tim3,
            &self.rcc,
            |w| w.tim3en().set_bit(),
            |w| w.tim3rst().clear_bit(),
        );
        configure_h_timer(
            timing,
            &self.mode_state.0.tim4,
            &self.rcc,
            |w| w.tim4en().set_bit(),
            |w| w.tim4rst().clear_bit(),
        );

        // Adjust tim3's CC2 value back in time.
        self.tim3.ccr2.modify(|r, w| w.ccr2().bits(
                r.ccr2().bits() - SHOCK_ABSORBER_SHIFT_CYCLES));

        // Configure tim3 to distribute its enable signal as its trigger output.
        self.tim3.cr2.write(|w| w
                       .mms().enable()
                       .ccds().clear_bit());

        let tim4 = &self.mode_state.0.tim4;

        // Configure tim4 to trigger from tim3 and run forever.
        tim4.smcr.write(|w| unsafe {
            // BUG: TS and SMS contents not modeled, have to use raw bits
            w.ts().bits(0b10)
                .sms().bits(0b110)
        });

        // Turn on tim4's interrupts.
        tim4.dier.write(|w| w
             .cc2ie().set_bit()    // Interrupt at start of active video.
             .cc3ie().set_bit());  // Interrupt at end of active video.

        // Turn on only one of tim3's:
        // Interrupt at start of active video.
        self.tim3.dier.write(|w| w.cc2ie().set_bit());

        // Note: timers still not running.

        // Initialize vsync output to its starting state.
        let gpiob = &self.mode_state.0.gpiob;
        match timing.vsync_polarity {
            Polarity::Positive => gpiob.bsrr.write(|w| w.br7().set_bit()),
            Polarity::Negative => gpiob.bsrr.write(|w| w.bs7().set_bit()),
        }

        /*
        TODO: since I've assigned ownership of the working buffer to the PendSV
        ISR, we can't do this here. I think this is okay, since it would only
        catch bugs in the very first rasterized scanline anyway.

        // Scribble over working buffer to help catch bugs.
        for pair in self.working_buffer().chunks_exact_mut(2) {
            pair[0] = 0xFF;
            pair[1] = 0x00;
        }
        */

        /*
        TODO: soooo I used to do this, but I'm pretty sure the rasterizer code
        will handle it, so.

        // Blank the final word of the scan buffer.
        for b in &mut self.scan_buffer()
                [timing.video_pixels .. timing.video_pixels + 4] {
            *b = 0
        }
        */

        // Set up global state.
        LINE.store(0, Ordering::Relaxed);
        *TIMING.try_lock().unwrap() = Some(timing.clone());
        VERT_STATE.store(VState::Blank as usize, Ordering::Relaxed);

        // This merely converts the sync pins to outputs; sync generation won't
        // start until the timers start below.
        sync_on(&self.mode_state.0.gpiob);

        // Reconstruct self in the new typestate, donating our hardware to the
        // ISRs.
        let hw = self.mode_state.0;
        *HSTATE_HW.try_lock().unwrap() = Some(hw);
        let mut new_self = Vga {
            rcc: self.rcc,
            flash: self.flash,
            gpioe: self.gpioe,
            tim3: self.tim3,
            nvic: self.nvic,
            mode_state: Sync(()),
        };

        // Turn on both our device interrupts (order doesn't matter)
        enable_irq(&mut new_self.nvic, device::Interrupt::TIM3);
        enable_irq(&mut new_self.nvic, device::Interrupt::TIM4);

        // Start TIM3, which starts TIM4.
        new_self.tim3.cr1.modify(|_, w| w.cen().set_bit());
        new_self
    }
}

/// Operations that are valid when sync has been configured, but before video
/// output is enabled.
impl Vga<Sync> {
    /// Provides `rast` to the driver interrupt handler as the raster callback,
    /// and executes `scope`. When `scope` returns, `rast` is revoked. Note that
    /// this may require busy-waiting until the end of active video.
    ///
    /// During the execution of `scope` the application has access to the driver
    /// in a different state, `Vga<Ready>`, which exposes additional operations.
    pub fn with_raster<R>(&mut self,
                          mut rast: impl for<'c> FnMut(usize,
                                                       &'c mut TargetBuffer,
                                                       &'c mut RasterCtx)
                                         + Send,
                          scope: impl FnOnce(&mut Vga<Live>) -> R)
        -> R
    {
        // We're punning our self reference for the other typestate below, so
        // make sure that's likely to work: (this assert should disappear)
        assert_eq!(core::mem::size_of::<Sync>(),
                   core::mem::size_of::<Live>());

        RASTER.donate(&mut rast, || {
            scope(unsafe { core::mem::transmute(self) })
        })
    }
}

impl Vga<Live> {
    /// Enables video output. This is not synchronized and can happen in the
    /// middle of the frame; if that bothers you, synchronize with vblank.
    pub fn video_on(&mut self) {
        // Disable pullups/pulldowns.
        self.gpioe.pupdr.modify(|_, w| w
                                .pupdr8().floating()
                                .pupdr9().floating()
                                .pupdr10().floating()
                                .pupdr11().floating()
                                .pupdr12().floating()
                                .pupdr13().floating()
                                .pupdr14().floating()
                                .pupdr15().floating());
        // Configure for very sharp edges. According to the reference manual
        // this sets the filter to 100MHz; at our 40MHz pixel clock this is an
        // improvement.
        self.gpioe.ospeedr.modify(|_, w| w
                                  .ospeedr8().very_high_speed()
                                  .ospeedr9().very_high_speed()
                                  .ospeedr10().very_high_speed()
                                  .ospeedr11().very_high_speed()
                                  .ospeedr12().very_high_speed()
                                  .ospeedr13().very_high_speed()
                                  .ospeedr14().very_high_speed()
                                  .ospeedr15().very_high_speed());
        // Configure for output.
        self.gpioe.moder.modify(|_, w| w
                                .moder8().output()
                                .moder9().output()
                                .moder10().output()
                                .moder11().output()
                                .moder12().output()
                                .moder13().output()
                                .moder14().output()
                                .moder15().output());
    }
}

pub fn init(mut nvic: cm::NVIC,
            scb: &mut cm::SCB,
            flash: device::FLASH,
            dbg: &device::DBG,
            rcc: device::RCC,
            gpiob: device::GPIOB,
            gpioe: device::GPIOE,
            tim1: device::TIM1,
            tim3: device::TIM3,
            tim4: device::TIM4,
            dma2: device::DMA2,
            )
    -> Vga<Idle>
{
    let previous_instance = DRIVER_INIT_FLAG.swap(true, Ordering::SeqCst);
    assert_eq!(previous_instance, false);

    // Turn on I/O compensation cell to reduce noise on power supply.
    rcc.apb2enr.modify(|_, w| w.syscfgen().enabled());
    // TODO: CMPCR seems to be modeled as read-only (?)
    //p.SYSCFG.cmpcr.modify(|_, w| w.cmp_pd().enabled());

    // Turn a bunch of stuff on.
    rcc.ahb1enr.modify(|_, w| w
                       .gpioben().enabled()
                       .gpioeen().enabled()
                       .dma2en().enabled());
    cortex_m::asm::dmb(); // ensure DMA is powered on before we write to it

    // DMA configuration.
    
    // Configure FIFO.
    dma2.s5fcr.write(|w| w
                     .fth().quarter()
                     .dmdis().enabled()
                     .feie().disabled());

    // Enable the pixel-generation timer.
    // We use TIM1; it's an APB2 (fast) peripheral, and with our clock config
    // it gets clocked at the full CPU rate.  We'll load ARR under rasterizer
    // control to synthesize 1/n rates.
    rcc.apb2enr.modify(|_, w| w.tim1en().enabled());
    tim1.psc.reset(); // Divide by 1 => PSC=0
    tim1.cr1.write(|w| w.urs().counter_only());
    tim1.dier.write(|w| w.ude().set_bit());

    // Configure interrupt priorities. This is safe because we haven't enabled
    // interrupts yet.
    unsafe {
        nvic.set_priority(device::Interrupt::TIM4, 0x00);
        nvic.set_priority(device::Interrupt::TIM3, 0x10);
        scb.set_priority(SystemHandler::PendSV, 0xFF);
    }

    // Enable Flash cache and prefetching to reduce jitter.
    flash.acr.modify(|_, w| w
                     .dcen().enabled()
                     .icen().enabled()
                     .prften().enabled());

    // Stop all video-related timers on debug halt, which makes debugging
    // waaaaay easier.
    dbg.dbgmcu_apb1_fz.modify(|_, w| w
                              .dbg_tim4_stop().set_bit()
                              .dbg_tim3_stop().set_bit());
    dbg.dbgmcu_apb2_fz.modify(|_, w| w
                              .dbg_tim1_stop().set_bit());

    let vga = Vga {
        rcc,
        flash,
        gpioe,
        nvic,
        tim3,
        mode_state: Idle(HStateHw {
            gpiob,
            tim1,
            tim4,
            dma2,
        }),
    };
    sync_off(&vga.mode_state.0.gpiob);
    vga.video_off();
    vga
}

/// Pattern for acquiring hardware resources loaned to an ISR in a static.
///
/// # Panics
///
/// If the `SpinLock` is locked when this is called. This would imply:
///
/// 1. that the IRQ got enabled too early, while the hardware is being
///    provisioned;
/// 2. That two ISRs are attempting to use the hardware without coordination.
/// 3. That a previous invocation of an ISR leaked the lock guard.
///
/// Also: if this is called before hardware is provisioned, implying that the
/// IRQ was enabled too early.
fn acquire_hw<T: Send>(lock: &SpinLock<Option<T>>) -> SpinLockGuard<T> {
    SpinLockGuard::map(
        lock.try_lock().expect("HW lock held at ISR"),
        |o| o.as_mut().expect("ISR fired without HW available"),
    )
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum VState {
    Blank = 0b00,
    Starting = 0b01,
    Active = 0b11,
    Finishing = 0b10,
}

impl VState {
    fn is_displayed_state(self) -> bool {
        (self as usize & 0b10) != 0
    }

    fn is_rendered_state(self) -> bool {
        (self as usize & 1) != 0
    }
}

static VERT_STATE: AtomicUsize = AtomicUsize::new(VState::Blank as usize);
static TIMING: SpinLock<Option<Timing>> = SpinLock::new(None);
static LINE: AtomicUsize = AtomicUsize::new(0);

fn vert_state() -> VState {
    match VERT_STATE.load(Ordering::Relaxed) {
        0b00 => VState::Blank,
        0b01 => VState::Starting,
        0b11 => VState::Active,
        _    => VState::Finishing,
    }
}

fn set_vert_state(s: VState) {
    VERT_STATE.store(s as usize, Ordering::Relaxed)
}

fn disable_h_timer(nvic: &mut cortex_m::peripheral::NVIC,
                   i: &device::Interrupt,
                   rcc: &device::RCC,
                   reset: impl FnOnce(&mut device::rcc::apb1rstr::W)
                               -> &mut device::rcc::apb1rstr::W) {
    disable_irq(nvic, copy_interrupt(i));
    rcc.apb1rstr.modify(|_, w| reset(w));
    cortex_m::asm::dsb();
    clear_pending_irq(copy_interrupt(i));
}

fn configure_h_timer(timing: &Timing,
                     tim: &device::tim3::RegisterBlock,
                     rcc: &device::RCC,
                     enable_clock: impl FnOnce(&mut device::rcc::apb1enr::W)
                               -> &mut device::rcc::apb1enr::W,
                     leave_reset: impl FnOnce(&mut device::rcc::apb1rstr::W)
                               -> &mut device::rcc::apb1rstr::W) {
    rcc.apb1enr.modify(|_, w| enable_clock(w));
    cortex_m::asm::dsb();
    rcc.apb1rstr.modify(|_, w| leave_reset(w));
    cortex_m::asm::dsb();

    // Configure the timer to count in pixels.  These timers live on APB1.
    // Like all APB timers they get their clocks doubled at certain APB
    // multipliers.
    let apb1_divisor = timing.clock_config.apb1_divisor.divisor();
    let apb_cycles_per_pixel = if apb1_divisor > 1 {
        (timing.cycles_per_pixel() * 2 / apb1_divisor)
    } else {
        timing.cycles_per_pixel()
    };

    // TODO PSC fields are defined as unsafe - BUG
    tim.psc.write(|w| unsafe { w.bits(apb_cycles_per_pixel as u32 - 1) });

    // TODO ARR fields are defined as unsafe - BUG
    tim.arr.write(|w| unsafe { w.bits(timing.line_pixels as u32 - 1) });

    tim.ccr1.write(|w| unsafe { w.bits(timing.sync_pixels as u32) });
    tim.ccr2.write(|w| unsafe { w.bits(
                (timing.sync_pixels
                 + timing.back_porch_pixels - timing.video_lead) as u32
                )});;
    tim.ccr3.write(|w| unsafe { w.bits(
                (timing.sync_pixels
                 + timing.back_porch_pixels + timing.video_pixels) as u32
                )});

    tim.ccmr1_output.write(|w| {
        unsafe { w.oc1m().bits(0b110); }  // PWM1 TODO
        unsafe { w.cc1s().bits(0b00); } // output TODO
        w
    });

    tim.ccer.write(|w| w
                   .cc1e().set_bit()
                   .cc1p().bit(timing.hsync_polarity == Polarity::Negative));
}

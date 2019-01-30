#![no_std]

pub mod timing;
pub mod rast;
pub mod util;

#[cfg(feature = "measurement")]
pub mod measurement;

mod startup;

#[allow(unused)] // TODO
mod font_10x16;

mod hstate;
mod bg_rast;
mod shock;

use stm32f4::stm32f407 as device;
use cortex_m::peripheral as cm;

use cortex_m::peripheral::scb::SystemHandler;

use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use crate::util::armv7m::{enable_irq, disable_irq, clear_pending_irq};
use crate::util::stm32::{UsefulDivisor, CopyHack, configure_clocks};
use crate::util::spin_lock::{SpinLock, SpinLockGuard};
use crate::rast::{RasterCtx, TargetBuffer};
use crate::timing::Polarity;

/// Representation of a pixel in memory.
pub type Pixel = u8;

/// Timing limitations mean we can't really pull off modes above 800x600, so
/// we'll use this fact to size some data structures.
pub const MAX_PIXELS_PER_LINE: usize = 800;

/// Raster ISR; needs to be called from `PendSV`.
pub use crate::bg_rast::maintain_raster_isr as pendsv_raster_isr;
/// Shock absorber ISR; needs to be called from `TIM3`.
pub use crate::shock::shock_absorber_isr as tim3_shock_isr;
/// Horizontal state machine ISR; needs to be called from `TIM4`.
pub use crate::hstate::hstate_isr as tim4_horiz_isr;

/// Driver state.
///
/// The `MODE` parameter tracks the state of the driver at compile time, and
/// changes what operations are available.
pub struct Vga<MODE> {
    rcc: device::RCC,
    flash: device::FLASH,
    gpioe: device::GPIOE,
    nvic: cm::NVIC,  // TODO probably should not own this

    mode_state: MODE,
}

/// Driver mode right after initialization, but before `configure_timing`.
pub struct Idle {
    hstate: HStateHw,
    tim3: device::TIM3,
}

/// Driver mode once timing has been configured.
pub struct Sync(());

/// Driver mode once rasterization has been configured.
pub struct Live(());

/// Trait for driver states where sync signals are being generated.
pub trait SyncOn {}

impl SyncOn for Sync {}
impl SyncOn for Live {}

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
        while LINE.load(Ordering::Relaxed) != 0 {
            cortex_m::asm::wfi()
        }
    }
}

/// Operations that are only valid before timing has been configured.
impl Vga<Idle> {
    /// Configures video timing.
    ///
    /// TODO: currently there is no way to re-configure timing, but the C++
    /// implementation supports that feature. The absence is a side effect of
    /// the type-state encoding of Vga. Provide a replacement.
    pub fn configure_timing(mut self, timing: &timing::Timing) -> Vga<Sync> {
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
        while self.mode_state.hstate.dma2.s5cr.read().en().bit_is_set() {
            // busy wait
        }

        // Switch to new CPU clock settings.
        configure_clocks(&self.rcc, &self.flash, &timing.clock_config);

        // Configure TIM3/4 for horizontal sync generation.
        configure_h_timer(
            timing,
            &self.mode_state.tim3,
            &self.rcc,
            |w| w.tim3en().set_bit(),
            |w| w.tim3rst().clear_bit(),
        );
        configure_h_timer(
            timing,
            &self.mode_state.hstate.tim4,
            &self.rcc,
            |w| w.tim4en().set_bit(),
            |w| w.tim4rst().clear_bit(),
        );

        // Adjust tim3's CC2 value back in time.
        self.mode_state.tim3.ccr2.modify(|r, w| w.ccr2().bits(
                r.ccr2().bits() - shock::SHOCK_ABSORBER_SHIFT_CYCLES));

        // Configure tim3 to distribute its enable signal as its trigger output.
        self.mode_state.tim3.cr2.write(|w| w
                       .mms().enable()
                       .ccds().clear_bit());

        let tim4 = &self.mode_state.hstate.tim4;

        // Configure tim4 to trigger from tim3 and run forever.
        tim4.smcr.write(|w| unsafe {
            // Safety: only unsafe due to upstream bug: SMCR register is
            // incompletely modeled. TODO
            w.ts().bits(0b10).sms().bits(0b110)
        });

        // Turn on tim4's interrupts.
        tim4.dier.write(|w| w
             .cc2ie().set_bit()    // Interrupt at start of active video.
             .cc3ie().set_bit());  // Interrupt at end of active video.

        // Turn on only one of tim3's:
        // Interrupt at start of active video.
        self.mode_state.tim3.dier.write(|w| w.cc2ie().set_bit());

        // Note: timers still not running.

        // Initialize vsync output to its starting state.
        let gpiob = &self.mode_state.hstate.gpiob;
        match timing.vsync_polarity {
            Polarity::Positive => gpiob.bsrr.write(|w| w.br7().set_bit()),
            Polarity::Negative => gpiob.bsrr.write(|w| w.bs7().set_bit()),
        }

        // Set up global state.
        LINE.store(0, Ordering::Relaxed);
        *TIMING.try_lock().unwrap() = Some(timing.clone());
        VERT_STATE.store(VState::Blank as usize, Ordering::Relaxed);

        // This merely converts the sync pins to outputs; sync generation won't
        // start until the timers start below.
        sync_on(&self.mode_state.hstate.gpiob);

        // Reconstruct self in the new typestate, donating our hardware to the
        // ISRs.
        let hw = self.mode_state.hstate;
        *HPSHARE.try_lock().unwrap() = Some(HPShared {
            hw,
            xfer: NextTransfer {
                dma_cr: device::dma2::s5cr::W::reset_value(),
                use_timer: false,
            },
        });
        let tim3 = self.mode_state.tim3;
        let mut new_self = Vga {
            rcc: self.rcc,
            flash: self.flash,
            gpioe: self.gpioe,
            nvic: self.nvic,
            mode_state: Sync(()),
        };

        // Start TIM3, which starts TIM4.
        tim3.cr1.modify(|_, w| w.cen().set_bit());
        *shock::SHOCK_TIMER.try_lock().unwrap() = Some(tim3);

        // Turn on both our device interrupts. We need to turn on TIM4 before
        // TIM3 or TIM3 may just wake up and idle forever.
        enable_irq(&mut new_self.nvic, device::Interrupt::TIM4);
        enable_irq(&mut new_self.nvic, device::Interrupt::TIM3);

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
                          mut rast: impl FnMut(usize,
                                               &mut TargetBuffer,
                                               &mut RasterCtx)
                                         + Send,
                          scope: impl FnOnce(&mut Vga<Live>) -> R)
        -> R
    {
        // We're punning our self reference for the other typestate below, so
        // make sure that's likely to work: (this assert should disappear)
        assert_eq!(core::mem::size_of::<Sync>(),
                   core::mem::size_of::<Live>());

        RASTER.donate(&mut rast, || {
            // Safety: I'm being super lazy here and punning a `Vga<Sync>`
            // reference for a `Vga<Live>` reference. This ought to hold because
            // they're both ZST.
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

/// Initializes the driver using the given hardware capabilities.
///
/// The driver is returned in `Idle` state, meaning output has not yet started.
/// You will likely want to call `configure_timing` followed by `with_raster`.
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

    #[cfg(feature = "measurement")]
    unsafe { measurement::init(); }

    let previous_instance = DRIVER_INIT_FLAG.swap(true, Ordering::SeqCst);
    assert_eq!(previous_instance, false);

    // Ensure that our interrupts are disabled.
    disable_irq(&mut nvic, device::Interrupt::TIM3);
    disable_irq(&mut nvic, device::Interrupt::TIM4);

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

    // Configure interrupt priorities.
    // Safety: messing with interrupt priorities is inherently unsafe, but we
    // disabled our device interrupts above and haven't pended a PendSV.
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
        mode_state: Idle {
            hstate: HStateHw {
                gpiob,
                tim1,
                tim4,
                dma2,
            },
            tim3,
        },
    };
    sync_off(&vga.mode_state.hstate.gpiob);
    vga.video_off();
    vga
}

/// Simplified version of `init` that assumes ownership of all hardware. This
/// covers the common case of pure graphics demos.
pub fn take_hardware() -> Vga<Idle> {
    let mut cp = cortex_m::peripheral::Peripherals::take().unwrap();
    let p = device::Peripherals::take().unwrap();

    init(
        cp.NVIC,
        &mut cp.SCB,
        p.FLASH,
        &p.DBG,
        p.RCC,
        p.GPIOB,
        p.GPIOE,
        p.TIM1,
        p.TIM3,
        p.TIM4,
        p.DMA2)
}

/// Records when a driver instance has been initialized. This is only allowed to
/// happen once at the moment because we don't have perfect teardown code.
static DRIVER_INIT_FLAG: AtomicBool = AtomicBool::new(false);

/// Data shared by the Hstate and PendSV ISRs.
struct HPShared {
    hw: HStateHw,
    xfer: NextTransfer,
}

/// SpinLock avoiding races between HState and PendSV.
static HPSHARE: SpinLock<Option<HPShared>> = SpinLock::new(None);

/// Hardware required by the horizontal state machine (and bits of it are shared
/// by PendSV, largely as an optimization).
struct HStateHw {
    dma2: device::DMA2,     // PendSV HState
    tim1: device::TIM1,     // PendSV HState
    tim4: device::TIM4,     //        HState
    gpiob: device::GPIOB,   //        HState
}

/// Groups parameters produced by PendSV for HState to consume, describing the
/// next DMA transfer.
struct NextTransfer {
    /// Contents of the DMA SxCR register value that starts the transfer.
    dma_cr: device::dma2::s5cr::W,
    /// Whether to use timer-mediated DMA to decrease horizontal resolution.
    use_timer: bool,
}

/// Shared copy of the current timing settings. This is shared between HState
/// and PendSV -- but it's locked at different times, and so stands separate.
static TIMING: SpinLock<Option<timing::Timing>> = SpinLock::new(None);

/// Storage for the raster callback reference. Loaded from thread mode, accessed
/// by hstate.
static RASTER: rast::IRef = rast::IRef::new();

/// Turns off sync outputs. This used to be public API, but I never use it, so.
fn sync_off(gpiob: &device::GPIOB) {
    gpiob.moder.modify(|_, w| w
                       .moder6().input()
                       .moder7().input());
    gpiob.pupdr.modify(|_, w| w
                       .pupdr6().pull_down()
                       .pupdr7().pull_down());
}

/// Turns on sync outputs.
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

/// Possible states of the vertical retrace state machine.
///
/// This is encoded as a Gray code for efficient testing by the functions below.
/// I haven't checked to see if that's actually efficient recently (TODO).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum VState {
    /// Deep in the vertical blanking interval.
    Blank = 0b00,
    /// On the line just before active video, so the rasterizer needs to be
    /// warming up.
    Starting = 0b01,
    /// Active video.
    Active = 0b11,
    /// On the final line in active video -- rasterizer must shut down but
    /// scanout will continue.
    Finishing = 0b10,
}

impl VState {
    /// Does scanout occur in this state?
    fn is_displayed_state(self) -> bool {
        (self as usize & 0b10) != 0
    }

    /// Does rasterization need to run in this state?
    fn is_rendered_state(self) -> bool {
        (self as usize & 1) != 0
    }
}

/// Non-blocking storage for the current vertical retrace state, encoded as a
/// `usize`. This is used by both ISRs.
static VERT_STATE: AtomicUsize = AtomicUsize::new(VState::Blank as usize);

/// Non-blocking storage for the current scan line number. Note that line
/// numbers used here start counting at the top of the vertical blanking
/// interval, not at the top of active video.
///
/// This is used by both ISRs, and also by applications to synchronize with
/// vertical retrace.
static LINE: AtomicUsize = AtomicUsize::new(0);

/// Convenient accessor for the vertical retrace state.
fn vert_state() -> VState {
    match VERT_STATE.load(Ordering::Relaxed) {
        0b00 => VState::Blank,
        0b01 => VState::Starting,
        0b11 => VState::Active,
        _    => VState::Finishing,
    }
}

/// Sets the vertical retrace state.
fn set_vert_state(s: VState) {
    VERT_STATE.store(s as usize, Ordering::Relaxed)
}

/// Utility for disabling one of our horizontal retrace timers. "Disabling" here
/// means that we ensure its interrupts cannot fire and it's left in reset.
fn disable_h_timer(nvic: &mut cortex_m::peripheral::NVIC,
                   i: &device::Interrupt,
                   rcc: &device::RCC,
                   reset: impl FnOnce(&mut device::rcc::apb1rstr::W)
                               -> &mut device::rcc::apb1rstr::W) {
    disable_irq(nvic, i.copy_hack());
    rcc.apb1rstr.modify(|_, w| reset(w));
    cortex_m::asm::dsb();
    clear_pending_irq(i.copy_hack());
}

/// Utility for configuring one of our horizontal retrace timers. It's set up
/// and taken out of reset, but its interrupts are not enabled.
fn configure_h_timer(timing: &timing::Timing,
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

    // Safety: only unsafe due to upstream BUG. TODO
    tim.psc.write(|w| unsafe { w.bits(apb_cycles_per_pixel as u32 - 1) });

    // Safety: only unsafe due to upstream BUG. TODO
    tim.arr.write(|w| unsafe { w.bits(timing.line_pixels as u32 - 1) });

    // Safety: only unsafe due to upstream BUG. TODO
    tim.ccr1.write(|w| unsafe { w.bits(timing.sync_pixels as u32) });
    // Safety: only unsafe due to upstream BUG. TODO
    tim.ccr2.write(|w| unsafe { w.bits(
                (timing.sync_pixels
                 + timing.back_porch_pixels - timing.video_lead) as u32
                )});
    // Safety: only unsafe due to upstream BUG. TODO
    tim.ccr3.write(|w| unsafe { w.bits(
                (timing.sync_pixels
                 + timing.back_porch_pixels + timing.video_pixels) as u32
                )});

    tim.ccmr1_output.write(|w| {
        // Safety: only unsafe due to upstream BUG. TODO
        unsafe { w.oc1m().bits(0b110); }  // PWM1
        // Safety: only unsafe due to upstream BUG. TODO
        unsafe { w.cc1s().bits(0b00); } // output
        w
    });

    tim.ccer.write(|w| w
                   .cc1e().set_bit()
                   .cc1p().bit(timing.hsync_polarity == Polarity::Negative));
}

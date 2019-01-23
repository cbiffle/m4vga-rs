pub mod rast;

use stm32f4::stm32f407 as device;
use cortex_m::peripheral::scb::SystemHandler;
use core::marker::PhantomData;
use core::sync::atomic::{AtomicBool, AtomicPtr, Ordering};
use core::cell::UnsafeCell;
use core::ptr::NonNull;

use crate::util::spin_lock::{SpinLock, SpinLockGuard};

use self::rast::RasterCtx;

pub type Pixel = u8;

pub const MAX_PIXELS_PER_LINE: usize = 800;

const SHOCK_ABSORBER_SHIFT_CYCLES: u32 = 20;

static mut DRIVER_INIT_FLAG: AtomicBool = AtomicBool::new(false);

#[derive(Copy, Clone, Debug)]
#[repr(align(4))]
struct WordAligned<T>(T);

#[link_section = ".local_ram"]
static mut GLOBAL_WORKING_BUFFER:
    WordAligned<[Pixel; rast::TARGET_BUFFER_SIZE]> =
    WordAligned([0; rast::TARGET_BUFFER_SIZE]);

#[link_section = ".scanout_ram"]
static mut GLOBAL_SCANOUT_BUFFER:
    WordAligned<[Pixel; rast::TARGET_BUFFER_SIZE]> =
    WordAligned([0; rast::TARGET_BUFFER_SIZE]);

#[derive(Copy, Clone, Debug)]
pub struct Timing {
    // TODO clock config

    /// Number of additional AHB cycles per pixel clock cycle. The driver uses a
    /// minimum of 4 cycles per pixel; this field adds to that. Larger values
    /// reduce both the resolution and the compute/bandwidth requirements.
    add_cycles_per_pixel: usize,

    /// Total horizontal pixels per line, including blanking.
    line_pixels: usize,
    /// Length of horizontal sync pulse.
    sync_pixels: usize,
    /// Number of pixels between end of sync and start of video.
    back_porch_pixels: usize,
    /// Fudge factor, nudging the DMA interrupt backwards in time to compensate
    /// for startup code taking non-zero time.
    video_lead: usize,
    /// Maximum visible pixels per line.
    video_pixels: usize,
    /// Polarity of horizontal sync pulse.
    hsync_polarity: Polarity,

    /// Scanline number of onset of vertical sync pulse, numbered from end of
    /// active video.
    vsync_start_line: usize,
    /// Scanline number of end of vertical sync pulse, numbered from end of
    /// active video.
    vsync_end_line: usize,
    /// Scanline number of start of active video, numbered from end of active
    /// video.
    video_start_line: usize,
    /// Scanline number of end of active video, numbered from end of active
    /// video in the last frame. This is the number of lines per frame.
    video_end_line: usize,
    /// Polarity of the vertical sync pulse.
    vsync_polarity: Polarity,
}

#[derive(Copy, Clone, Debug)]
pub enum Polarity {
    Positive = 0,
    Negative = 1,
}

pub struct Vga<MODE> {
    gpiob: device::GPIOB,
    gpioe: device::GPIOE,
    tim1: device::TIM1,
    tim3: device::TIM3,
    tim4: device::TIM4,
    dma2: device::DMA2,

    _marker: PhantomData<MODE>,
}

#[derive(Debug)]
pub enum Idle {}

#[derive(Debug)]
pub enum Ready {}

static RASTER: rast::IRef = rast::IRef::new();

impl<T> Vga<T> {
    fn working_buffer(&self) -> &'static mut [Pixel; rast::TARGET_BUFFER_SIZE] {
        unsafe {
            &mut GLOBAL_WORKING_BUFFER.0
        }
    }

    fn scan_buffer(&self) -> &'static mut [Pixel; rast::TARGET_BUFFER_SIZE] {
        unsafe {
            &mut GLOBAL_SCANOUT_BUFFER.0
        }
    }

    /// Busy-waits for the transition from active video to vertical blank.
    /// Because this waits for the *transition*, if you call this *during*
    /// vblank it will wait for an entire frame.
    pub fn sync_to_vblank(&self) {
        unimplemented!()
    }

    pub fn sync_on(&self) {
        // Configure PB6/PB7 for fairly sharp edges.
        self.gpiob.ospeedr.modify(|_, w| w
                                  .ospeedr6().high_speed()
                                  .ospeedr7().high_speed());
        // Disable pullups/pulldowns.
        self.gpiob.pupdr.modify(|_, w| w
                                .pupdr6().floating()
                                .pupdr7().floating());
        // Configure PB6 as AF2 and PB7 as output.
        self.gpiob.afrl.modify(|_, w| w.afrl6().af2());
        self.gpiob.moder.modify(|_, w| w
                                .moder6().alternate()
                                .moder7().output());
    }

    pub fn sync_off(&self) {
        self.gpiob.moder.modify(|_, w| w
                                .moder6().input()
                                .moder7().input());
        self.gpiob.pupdr.modify(|_, w| w
                                .pupdr6().pull_down()
                                .pupdr7().pull_down());
    }

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

impl Vga<Idle> {
    /// Provides `rast` to the driver interrupt handler as the raster callback,
    /// and executes `scope`. When `scope` returns, `rast` is revoked. Note that
    /// this may require busy-waiting until the end of active video.
    ///
    /// During the execution of `scope` the application has access to the driver
    /// in a different state, `Vga<Ready>`, which exposes additional operations.
    pub fn with_raster<R>(&mut self,
                          mut rast: impl for<'c> FnMut(usize, &'c mut RasterCtx)
                                         + Send,
                          scope: impl FnOnce(&mut Vga<Ready>) -> R)
        -> R
    {
        RASTER.donate(&mut rast, || {
            scope(unsafe { core::mem::transmute(self) })
        })
    }

    /// Configures video timing. This is only available when we aren't actively
    /// rasterizing.
    pub fn configure_timing(&mut self, timing: &Timing) {
        // TODO: timing consistency asserts

        self.video_off();
        self.sync_off();

        // Place the horizontal timers in reset, disabling interrupts.
        //disable_h_timer(ApbPeripheral::tim4, Interrupt::tim4);
        //disable_h_timer(ApbPeripheral::tim3, Interrupt::tim3);

        // Busy-wait for pending DMA to complete.
        while self.dma2.s5cr.read().en().bit_is_set() {
            // busy wait
        }

        // Switch to new CPU clock settings.
        //rcc.configure_clocks(timing.clock_config);

        // Configure TIM3/4 for horizontal sync generation.
        //configure_h_timer(timing, ApbPeripheral::tim3, tim3);
        //configure_h_timer(timing, ApbPeripheral::tim4, tim4);

        // Adjust tim3's CC2 value back in time.
        self.tim3.ccr2.modify(|r, w| w.ccr2().bits(
                r.ccr2().bits() - SHOCK_ABSORBER_SHIFT_CYCLES));

        // Configure tim3 to distribute its enable signal as its trigger output.
        self.tim3.cr2.write(|w| w
                       .mms().enable()
                       .ccds().clear_bit());

        // Configure tim4 to trigger from tim3 and run forever.
        self.tim4.smcr.write(|w| unsafe {
            // BUG: TS and SMS contents not modeled, have to use raw bits
            w.ts().bits(0b10)
                .sms().bits(0b110)
        });


        // Turn on tim4's interrupts.
        self.tim4.dier.write(|w| w
                  .cc2ie().set_bit()    // Interrupt at start of active video.
                  .cc3ie().set_bit());  // Interrupt at end of active video.

        // Turn on only one of tim3's:
        // Interrupt at start of active video.
        self.tim3.dier.write(|w| w.cc2ie().set_bit());

        // Note: timers still not running.

        // Initialize vsync output to its starting state.
        match timing.vsync_polarity {
            Polarity::Positive => self.gpiob.bsrr.write(|w| w.br7().set_bit()),
            Polarity::Negative => self.gpiob.bsrr.write(|w| w.bs7().set_bit()),
        }

        // Scribble over working buffer to help catch bugs.
        for pair in self.working_buffer().chunks_exact_mut(2) {
            pair[0] = 0xFF;
            pair[1] = 0x00;
        }

        // Blank the final word of the scan buffer.
        for b in &mut self.scan_buffer()[timing.video_pixels .. timing.video_pixels + 4] {
            *b = 0
        }

        /*

  // Set up global state.
  current_line = 0;
  current_timing = timing;
  state = State::blank;
  working_buffer_shape = {
    .offset = 0,
    .length = 0,
    .cycles_per_pixel = timing.cycles_per_pixel,
    .repeat_lines = 0,
  };
  next_use_timer = false;

  scan_buffer_needs_update = false;

  // Start TIM3, which starts TIM4.
  enable_irq(Interrupt::tim3);
  enable_irq(Interrupt::tim4);
  tim3.write_cr1(tim3.read_cr1().with_cen(true));

  sync_on();
  */

    }
}

impl Vga<Ready> {
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

fn a_test(vga: &mut Vga<Idle>) -> ! {
    let mut color = 0;
    vga.with_raster(
        |ln, rc| {
            rast::solid_color_fill(ln, rc, 800, color);
            color = color.wrapping_add(1);
        },
        |vga| {
            loop { }
        },
    )
}

pub fn init(cp: &mut device::CorePeripherals,
            rcc: &device::RCC,
            flash: &device::FLASH,
            dbg: &device::DBG,
            gpiob: device::GPIOB,
            gpioe: device::GPIOE,
            tim1: device::TIM1,
            tim3: device::TIM3,
            tim4: device::TIM4,
            dma2: device::DMA2,
            )
    -> Vga<Idle>
{
    let previous_instance = unsafe {
        DRIVER_INIT_FLAG.swap(true, Ordering::SeqCst)
    };
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

    // DMA configuration.
    
    // Configure FIFO.
    dma2.s5fcr.write(|w| w
                     .fth().quarter()
                     .dmdis().enabled()
                     .feie().disabled());
    // Enable the pixel-generation timer.
    rcc.apb2enr.modify(|_, w| w.tim1en().enabled());
    tim1.psc.reset(); // Divide by 1 => PSC=0
    tim1.cr1.write(|w| w.urs().counter_only());
    tim1.dier.write(|w| w.ude().set_bit());

    // Configure interrupt priorities. This is safe because we haven't enabled
    // interrupts yet.
    unsafe {
        cp.NVIC.set_priority(device::Interrupt::TIM4, 0x00);
        cp.NVIC.set_priority(device::Interrupt::TIM3, 0x10);
        cp.SCB.set_priority(SystemHandler::SysTick, 0xFF);
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
        gpiob,
        gpioe,
        tim1,
        tim3,
        tim4,
        dma2,
        _marker: PhantomData,
    };
    vga.sync_off();
    vga.video_off();
    vga
}

static SHOCK_TIMER: SpinLock<Option<device::TIM3>> = SpinLock::new(None);

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

pub fn shock_absorber_isr() {
    let mut tim = acquire_hw(&SHOCK_TIMER);
    tim.sr.modify(|_, w| w.cc2if().clear_bit());
    cortex_m::asm::wfi()
}

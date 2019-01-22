pub mod rast;

use stm32f4::stm32f407 as device;
use cortex_m::peripheral::scb::SystemHandler;
use core::marker::PhantomData;
use core::sync::atomic::{AtomicBool, AtomicPtr, Ordering};
use core::cell::UnsafeCell;
use core::ptr::NonNull;

use crate::util::spin_lock::SpinLock;

use self::rast::RasterCtx;

pub type Pixel = u8;

pub const MAX_PIXELS_PER_LINE: usize = 800;

#[derive(Copy, Clone, Debug)]
pub enum NextFrame {
    Same,
    Release,
}

#[derive(Debug)]
pub struct Vga<MODE>(PhantomData<MODE>);

#[derive(Debug)]
pub enum Idle {}

#[derive(Debug)]
pub enum Ready {}

static RASTER: rast::IRef = rast::IRef::new();

impl<T> Vga<T> {
    /// Busy-waits for the transition from active video to vertical blank.
    /// Because this waits for the *transition*, if you call this *during*
    /// vblank it will wait for an entire frame.
    pub fn sync_to_vblank(&self) {
        unimplemented!()
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
            scope(&mut Vga(PhantomData))
        })
    }
}

impl Vga<Ready> {
    /// Enables video output. This is not synchronized and can happen in the
    /// middle of the frame; if that bothers you, synchronize with vblank.
    pub fn video_on(&mut self) {}
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
            p: &device::Peripherals)
    -> Vga<Idle>
{
    // Turn on I/O compensation cell to reduce noise on power supply.
    p.RCC.apb2enr.modify(|_, w| w.syscfgen().enabled());
    // TODO: CMPCR seems to be modeled as read-only (?)
    //p.SYSCFG.cmpcr.modify(|_, w| w.cmp_pd().enabled());

    // Turn a bunch of stuff on.
    p.RCC.ahb1enr.modify(|_, w| w
                         .gpioben().enabled()
                         .gpioeen().enabled()
                         .dma2en().enabled());

    // DMA configuration.
    
    // Configure FIFO.
    p.DMA2.s5fcr.write(|w| w
                       .fth().quarter()
                       .dmdis().enabled()
                       .feie().disabled());
    // Enable the pixel-generation timer.
    p.RCC.apb2enr.modify(|_, w| w.tim1en().enabled());
    p.TIM1.psc.reset(); // Divide by 1 => PSC=0
    p.TIM1.cr1.write(|w| w.urs().counter_only());
    p.TIM1.dier.write(|w| w.ude().set_bit());

    // Configure interrupt priorities. This is safe because we haven't enabled
    // interrupts yet.
    unsafe {
        cp.NVIC.set_priority(device::Interrupt::TIM4, 0x00);
        cp.NVIC.set_priority(device::Interrupt::TIM3, 0x10);
        cp.SCB.set_priority(SystemHandler::SysTick, 0xFF);
    }

    // Enable Flash cache and prefetching to reduce jitter.
    p.FLASH.acr.modify(|_, w| w
                       .dcen().enabled()
                       .icen().enabled()
                       .prften().enabled());

    // TODO more

    Vga(PhantomData)
}

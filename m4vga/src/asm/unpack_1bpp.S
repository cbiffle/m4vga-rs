.syntax unified
.section .ramcode,"ax",%progbits

@ Unpacks 1bpp packed pixel data into an 8bpp scan buffer, using a two-color
@ lookup table.
@
@ Arguments:
@  r0  start of input line containing 1bpp packed pixels (word-aligned)
@  r1  CLUT pointer to two bytes: the zero color and high color.
@  r2  output scan buffer.
@  r3  width of input line in words.
@
@ The implementation uses a trick. We process pixels in groups of four, by
@ copying the packed pixel bits into the vector comparison result flags field
@ (GE) of the PSR. From there, we can use the SEL instruction (byte select) to
@ take each output byte from one of two registers, depending on the pixel bit.
@
@ This nets out to just over 1 cycle per pixel. This is the key enabling hack
@ that allows us to do interesting math while displaying high-resolution
@ graphics.
@
@ All cycle counts annotated below have been empirically verified.
.global unpack_1bpp_impl
.balign 4
.thumb_func
unpack_1bpp_impl:
      @ Name the arguments...
      framebuffer .req r0
      clut        .req r1
      target      .req r2
      words       .req r3

      @ Name temporaries...
      vclut0      .req r4
      vclut1      .req r5
      bits        .req r6
      tmp         .req r7

      @ Actual code from here:                                          Cycles

      stmdb.w sp!, { vclut0, vclut1, bits, tmp }  @ Free up registers.      5
      @ (using wide form to preserve 32-bit alignment)

      @ Prepare vector CLUTs in callee-save registers.

      @ Our use of GE and SEL requires that the CLUTs be prepared in a
      @ particular way: we need one register per color, and the color
      @ must be copied into each byte of the register.

      @ Load the colors for each pixel. We could load them in one LDR, yes,
      @ but the math below requires the top 24 bits to be clear anyway, so
      @ this winds up being slightly faster thanks to load pipelining.
      ldrb vclut0, [clut]               @ Zero color                        2
      ldrb vclut1, [clut, #1]           @ One color                         1

      @ Replicate each color's byte into all four byte lanes.
      @ Because the top 24 bits of each register are clear, we can do this
      @ with multiplication by a repeating bit pattern. Both loading a
      @ repeating bit pattern and multiplication are very cheap on this
      @ architecture!
      mov clut, #0x01010101             @ Magic byte-lane smear constant.   1
      muls vclut0, clut                 @ Make vectors                      1
      muls vclut1, clut                 @ out of each entry.                1

      @ The register formerly known as clut changes roles here...
      colors .req clut
      .unreq clut

      @ Total cycles for startup:                                          11

      @ Enough paperwork. Start unpacking!
      .balign 4  @ Should be aligned anyway, but ensure it.
0:    ldr bits, [framebuffer], #4       @ Load a block of 32 pixels.        2

      @ Process four bits as a unit. The lsb=0 case must be run last,
      @ because it destructively modifies 'target' with a post-increment
      @ store. (We don't post-increment every time because it's a cycle
      @ slower on Cortex-M4.) The lsb=16 case should be run first, to best
      @ align the rest.
      @
      @ NOTE: the 'msr APSR_g, rX' instruction copies from bits 19:16 of
      @ the source register, *not* the LSBs!
      @
      @ Assembles to:
      @ - 14 bytes / 4 cycles when lsb=0.
      @ - 10 bytes / 3 cycles when lsb=16.
      @ - 12 bytes / 4 cycles otherwise.
      .macro STEP lsb
        .if (\lsb - 16)
          @ Shift the bits into position at 19:16.
          .ifgt (\lsb - 16)
            lsrs tmp, bits, #(\lsb - 16)
          .else
            lsls tmp, bits, #(16 - \lsb)
          .endif
          @ Load GE.
          msr APSR_g, tmp
        .else
          @ They're already in position, just load
          msr APSR_g, bits
        .endif
        sel colors, vclut1, vclut0    @ Use it to mux colors.       1
        .if \lsb
          str colors, [target, #\lsb]                             @ 1
        .else
          str colors, [target], #32                               @ 1
        .endif
      .endm

      @ See comment above for why these are in a strange order.
      STEP 16                                                             @ 3
      STEP 4                                                              @ 4
      STEP 8                                                              @ 4
      STEP 12                                                             @ 4
      STEP 20                                                             @ 4
      STEP 24                                                             @ 4
      STEP 28                                                             @ 4
      STEP 0                                                              @ 4

      subs words, #1                                                      @ 1
      bhi 0b                                                              @ 2/1

      @ Total cycles for loop body:                                        36/35

      @ Aaaaaand we're done.
      pop { vclut0, vclut1, bits, tmp }                                   @ 5
      bx lr                                                               @ 2

      @ Cycles for 800-pixel line, including return: 917, or 1.146 c/p (3.49x
      @ realtime) using CCM; 942 (3.40x realtime) using SRAM112 due to fetch
      @ contention.

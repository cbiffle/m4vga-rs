.syntax unified
.section .ramcode,"ax",%progbits

@ Generates a scrolling procedural texture using the traditional row-column XOR
@ pattern.  To make the texture visible, it's generated in four-pixel units,
@ but scrolled in single pixel increments for smoothness.
@
@ To implement scrolling, the Mode wrapper adjusts the line and column numbers
@ given to the assembly routine.

@ Inputs:
@  r0  line number (seed for pattern on Y axis)
@  r1  column number (seed for pattern on X axis)
@  r2  raster target
@  r3  number of bytes in raster target.
.global xor_pattern_impl
.thumb_func
xor_pattern_impl:
      @ Name inputs
      line  .req r0
      col   .req r1
      out   .req r2
      count .req r3

      @ Free up and name temporaries
      tmp   .req r4
      acc   .req r5
      cba0  .req r6
      push {tmp, acc, cba0}

      @ We're going to lean on the SIMD instruction set pretty heavily to do
      @ this efficiently.  A naive implementation would take several cycles
      @ per pixel, and would likely do far worse than real-time.

      @ We'll produce pixels in groups of four.  From left to right, at a given
      @ col and line, the pixels are:
      @
      @  (col >> 2) ^ line
      @  ((col + 1) >> 2) ^ line
      @  ((col + 2) >> 2) ^ line
      @  ((col + 3) >> 2) ^ line
      @
      @ We can turn this inside out by recognizing that the (c+n)>>2 pattern
      @ is equivalent to right-shifting and then adding one to between zero and
      @ three of the pixels:
      @
      @  ((col >> 2) + 0) ^ line
      @  ((col >> 2) + a) ^ line
      @  ((col >> 2) + b) ^ line
      @  ((col >> 2) + c) ^ line
      @
      @ a, b, and c are set based on col[1:0]:
      @
      @  col[1:0]  a  b  c
      @    00      0  0  0
      @    01      0  0  1
      @    10      0  1  1
      @    11      1  1  1
      @
      @ Since we'll increment col by 4 after generating four pixels, col[1:0]
      @ are actually invariant throughout the scanline.  So we can precalculate
      @ a, b, and c.  To use SIMD, we calculate them as a byte vector cba0.
      @
      @ Since (c + 4) >> 2 == (c >> 2) + 1, we can also pre-shift col by two
      @ places.

      @ Calculate cba0.
      ldr cba0, =0x01010100     @ Prepare most aggressive value.
      ubfx tmp, col, #0, #2     @ Extract col[1:0]
      eor tmp, tmp, #3          @ Invert it.
      lsls tmp, #3              @ Multiply by 8.
      lsls cba0, tmp            @ Shift cba0 to increment proper MSBs.

      @ Pre-shift col and clear 24 MSBs.
      @ This is equivalent to extracting bits 9:2.
      ubfx col, col, #2, #8

      @ Clear top 24 bits of line.
      uxtb line, line

      @ Byte-lane replication factor.
      mov tmp, #0x01010101

      @ Replicate col and line into vectors.
      mul line, line, tmp
      mul col, col, tmp

      .balign 4   @ Saves about a cycle per iteration.
0:    @ Produce a batch of four pixels.
      uadd8 acc, col, cba0    @ Compute ((c>>2)+n) for each lane.
      uadd8 col, col, tmp     @ Advance each col replica by 1.
      eor   acc, acc, line    @ Take line into account.
      str acc, [out], #4      @ Write out batch.
      subs count, #4          @ Decrement pixel counter.
      bhi 0b                  @ Repeat while there are pixels.

      pop {tmp, acc, cba0}
      bx lr

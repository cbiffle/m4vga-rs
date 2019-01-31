.syntax unified
.section .ramcode,"ax",%progbits

@ Rasterizes 256-color text with per-character colors, using a bitmap font.
@
@ Input
@ -----
@
@ The input buffer is a sequence of 32-bit words, one per character.  Each
@ word contains...
@  Bit
@   7: 0  8-bit character (font index).
@  15: 8  Background color.
@  23:16  Foreground color.
@  31:24  Attributes (currently unused).
@
@ Font
@ ----
@
@ The font is used as a lookaside table for translating 8-bit characters into
@ groups of pixels.  The font contains 8-pixel wide glyphs for each character,
@ where a 1 bit indicates the foreground color, and a 0 bit indicates the
@ background color.
@
@ Fonts are stored row-normal: first the y=0 row of every glyph, then the y=1
@ rows, and so on.  This makes indexing cheaper for this implementation.
@
@ The rasterizer must determine which row of the glyph is being drawn and offset
@ the font pointer accordingly.  This means that this unpacker can be used,
@ without change, for fonts with 1-256 glyphs of arbitrary height.
@
@ Output
@ ------
@
@ Characters are drawn 10 pixels wide, of which 8 pixels are read from the font,
@ and the remaining 2 provide inter-character spacing (the "gutter").
@
@ You may have noticed that 10 is not a multiple of four.  To write the pixel
@ output most efficiently, we could handle pairs of characters, writing them
@ out in 5-word (20-pixel) groups.  This is sufficiently elaborate that it's
@ actually cheaper to take the penalty cycles for unaligned access.
@
@ The implementation is very similar to the 1bpp unpacker, just with a CLUT
@ that changes every 10 pixels.

@ Inputs:
@  r0  input line.
@  r1  font row pointer.
@  r2  output raster target.
@  r3  number of characters to process.
@
.global unpack_text_10p_attributed_impl
.thumb_func
unpack_text_10p_attributed_impl:
      @ Name the inputs
      text    .req r0
      font    .req r1
      target  .req r2
      cols    .req r3

      @ Free up and name some working registers.
      fore    .req r4
      back    .req r5
      lsbs    .req r6
      bits    .req r7
      color0  .req r8

      push.w {fore, back, lsbs, bits, color0}  @ Wide to maintain alignment.

      @ This constant is used to smear colors across byte lanes, because
      @ ARMv7-M doesn't have vector shuffle operations.
      mov.w lsbs, #0x01010101

      @ Get on with it!
      .balign 4
0:    @ Load an attributed character into 'bits'.
      @ (This load cannot pipeline with the next because of the address
      @ dependency, so there's no need to pack 'em.)
      ldr bits, [text], #4                                            @ 2

      @ Extract colors and character into separate registers.
      @ "bits" will hold the character.
      uxtb fore, bits, ROR #16                                        @ 1
      uxtb back, bits, ROR #8                                         @ 1
      uxtb bits, bits                                                 @ 1

      @ Smear colors across byte lanes.
      muls fore, lsbs                                                 @ 1
      muls back, lsbs                                                 @ 1

      @ Load a row of glyph data from the font.
      ldrb bits, [font, bits]                                         @ 2

      @ Mux fore and back to produce combined colors for each glyph pixel.
      @ We use the same approach as the 1bpp unpacker: stuffing glyph bits
      @ into the GE field of the PSR and using the sel instruction.
      @ First, shift the glyph bits so the LSBs are in 19:16.
      @ The high-order bits are preserved in 23:20.
      lsls bits, #16                                                  @ 1
      msr APSR_g, bits                                                @ 1
      sel color0, fore, back                                          @ 1

      @ Now do it again.
      lsrs bits, #4                                                   @ 1
      msr APSR_g, bits                                                @ 1
      sel bits, fore, back    @ bits now holds pixels                   1

      @ Store ten pixels: the eight we just generated, and the two-pixel gutter.
      @ Prefer displacement addressing to postincrement to avoid an address
      @ generation stall (also improves code density but to no measurable
      @ effect).
      @
      @ This may look lke a good candidate for the STMIA instruction, but
      @ that instruction requires aligned memory accesses, which we don't
      @ guarantee here.
      str bits, [target, #4]                                          @ 1
      strh back, [target, #8]                                         @ 1 / 2
      str color0, [target], #10                                       @ 2 / 3

      @ Advance column.  Yes, the APSR output of this instruction is consumed
      @ by the immediately following branch.  No, this does not appear to
      @ cause a stall, nor does hoisting this instruction higher improve
      @ performance.
      subs cols, #1                                                   @ 1

      @ Aaaand repeat.
      bne 0b                                                          @ 2

      pop {fore, back, lsbs, bits, color0}
      bx lr

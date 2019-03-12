.syntax unified

#ifdef __ARM_PCS_VFP

@ High-throughput block transfer using the FPU register set as a 128-byte
@ buffer.
@
@ Arguments:
@  r0  source address
@  r1  destination address
@  r2  number of words to transfer.
@
@ The primary trick being used here: the LDM/STM instructions generate repeated
@ memory transactions without stopping to fetch new instructions. The integer
@ LDM/STM can produce up to 16 transactions (though in practice they're limited
@ to 15 because one of those transactions would effect a jump). The floating
@ point VLDM/VSTM, on the other hand, can move up to 32 words / 128 bytes per
@ instruction fetched.
@
@ So, we clear out the FP register file and funnel data through it.
@
@ Against zero-wait-state memory, this is about twice as fast as the DMA
@ controller on STM32F4.
.section .ramcode,"ax",%progbits
.balign 4 @ Make sure we start out aligned.
.global copy_words_impl
.thumb_func
copy_words_impl:
      @ Name our registers.
      src   .req r0
      dst   .req r1
      count .req r2

      @ Empirical cycle counts in column at right.

      @ The caller may have been using floating point. Save the callee-save
      @ portion of the register file.
      vpush {s16 - s31}                                           @ 17

      @ "Warm up" the transfer engine, which wants to operate in units of
      @ 128 bytes, by making smaller transfers until 'count' is a multiple of
      @ 128.
      @
      @ In the warm-up phase, we exploit the Cortex-M4's IT Folding feature.
      @ An IT instruction following a 16-bit Thumb instruction takes no
      @ additional cycles to execute, when both are packed into an aligned
      @ 32-bit word.

      @ Special-case the single word transfer; the macro below won't work.
      lsrs.n count, #1                                            @ 1
      itt cs                                                      @ 0 (aligned)
      vldmcs.32 src!, {s0}                                        @ 2
      vstmcs.32 dst!, {s0}                                        @ 2

      @ Transfer n+1 words.
      .macro XFER n                             @ 5 + 2*n
        lsrs.n count, #1                        @ 1
        itt cs                                  @ 0 (aligned)
        vldmcs.32 src!, {s0 - s\n}              @ 1+1+n
        vstmcs.32 dst!, {s0 - s\n}              @ 1+1+n
      .endm

      XFER 1                                                      @ 7
      XFER 3                                                      @ 11
      XFER 7                                                      @ 19
      XFER 15                                                     @ 35

      @ Handle the case where we've been asked to transfer <32 words.
      @ In such a case, 'count' will now be zero, and the Z flag will still
      @ be set from the last XFER.
      @
      @ Force the branch to use a 32-bit instruction to preserve alignment
      @ of the loop branch below; this saves a cycle per loop iteration.
      @
      @ Note that the target of this branch (at 1 below) is also aligned,
      @ saving a cycle on the rare escape path.
      beq.w 1f                                                    @ 1 (n.t.)

      @ All warmed up; transfer in units of 128 bytes. Note the explicit use of
      @ 16-bit (.n) instructions to maintain loop alignment.
0:    vldm.32 src!, {s0 - s31}                                    @ 33
      vstm.32 dst!, {s0 - s31}                                    @ 33
      subs.n count, #1                                            @ 1
      bne.n 0b                                                    @ ~3 (taken)

      @ Restore FPU state.
1:    vpop {s16 - s31}                                            @ 17
      bx lr                                                       @ 1-3??

#else
  #error copy_words is not available for your architecture.
#endif

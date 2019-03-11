.syntax unified

@ Fills a contiguous span of bytes with a repeating byte.  This is exactly
@ equivalent to memset.
.section .ramcode.fill,"ax",%progbits
.balign 4
.global fast_fill
.thumb_func
fast_fill:
        @ Arguments
        ptr   .req r0
        end   .req r1
        value .req r2

        @ Temporaries
        count .req r3
        tmp   .req r4

        push {r4}

        .balign 4

        mov tmp, #0x01010101              @ Magical byte-lane smear constant.
        mul value, tmp                    @ Replicate byte four times.

        @ If we're transferring a small number of bytes, the code below
        @ may run past the end.  Special-case this.
        subs count, end, ptr              @ Derive byte count.
        cmp count, #8
        blo 99f

        @ Perform initial transfers to align to a word boundary.
        lsrs tmp, ptr, #1                 @ Shift ptr[0] into C.
        it cs                             @ If it was 1,
        strbcs value, [ptr], #1           @ store out a byte and increment.

        lsrs tmp, ptr, #2                 @ Shift ptr[1] into C.
        it cs                             @ If it was 1,
        strhcs value, [ptr], #2           @ store out a halfword and increment.

        subs count, end, ptr              @ Recompute byte count.

        @ Move as many words as we can.  This is an unrolled transfer that
        @ avoids postincrement stores, which cost more on Cortex-M4.

        bic count, #0x3                   @ Round count down to words.
        adr tmp, 0f                       @ Get address of *end* of unroll.
        subs tmp, count                   @ Start somewhere inside it.
        mov pc, tmp
        @ Note: the sub instruction could directly address PC, per the ARM,
        @ but Binutils doesn't agree.
       
        .set ctr, 800
        .rept 200
        .set ctr, ctr - 4
        str.w value, [ptr, #ctr]
        .endr

        @ Handle the trailing bytes -- there are up to three.
        @ We've failed to advance ptr, above, but we can work backwards from
        @ end.
0:      
        lsrs tmp, end, #1                 @ Shift end[0] into C.
        it cs                             @ If it is 1,
        strbcs value, [end, #-1]!         @ write the final byte.

        lsrs tmp, end, #2                 @ Shift end[1] into C.
        it cs                             @ If it is 1,
        strhcs value, [end, #-2]          @ write the final halfword.


1:      pop {r4}
        bx lr                             @ All done.

        .balign 4
99:     cmp ptr, end
        beq 1b

0:      strb value, [ptr], #1
        cmp ptr, end
        bne 0b
        b 1b

.section .text.start
.globl _start
_start:
    la sp, __stack_top
    call main
    # If main returns, loop forever
1:  j 1b

.section .data
.align 4
__stack_top = 0x80100000

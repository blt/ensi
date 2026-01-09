ENTRY(_start)
MEMORY {
  RAM (rwx) : ORIGIN = 0x80000000, LENGTH = 1M
}
SECTIONS {
  .text : { *(.text.start) *(.text*) } > RAM
  .rodata : { *(.rodata*) } > RAM
  .data : { *(.data*) } > RAM
  .bss : { *(.bss*) *(COMMON) } > RAM
  /DISCARD/ : { *(.eh_frame*) *(.note*) *(.comment*) }
}

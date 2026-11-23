/* The test boot core: RAM only, inside what an app may load (below
 * stage0's window) and apart on both buses: code at IRAM 0x40380000 (the
 * SRAM1 the data bus sees at 0x3FC90000), data at DRAM 0x3FCA0000. */
INCLUDE rom.x
MEMORY
{
  iram (RX) : ORIGIN = 0x40380000, LENGTH = 0x8000
  dram (RW) : ORIGIN = 0x3FCA0000, LENGTH = 0x4000
}
ENTRY(core_start)
SECTIONS
{
  .text : ALIGN(4) { *(.literal .text .literal.* .text.*) } > iram
  .rodata : ALIGN(4) { *(.rodata .rodata.*) } > dram
  .data : ALIGN(4) { *(.data .data.*) } > dram
  .bss (NOLOAD) : ALIGN(4) { *(.bss .bss.* COMMON) } > dram
  /DISCARD/ : { *(.xt.prop .xt.prop.* .xt.lit .xt.lit.* .xtensa.info .comment) }
}

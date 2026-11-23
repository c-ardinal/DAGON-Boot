/* The test app: code on the instruction bus (IROM) and strings on the data
 * bus (DROM), both read from flash through the MMU that stage0 sets up,
 * on the MMU entries an esp-hal app takes (DROM entry 0, IROM entry 1). */
INCLUDE rom.x
MEMORY
{
  irom (RX) : ORIGIN = 0x42010020, LENGTH = 0x8000
  drom (R)  : ORIGIN = 0x3C000020, LENGTH = 0x8000
}
ENTRY(app_start)
SECTIONS
{
  .text : ALIGN(4) { *(.literal .text .literal.* .text.*) } > irom
  .rodata : ALIGN(4) { *(.rodata .rodata.*) } > drom
  /DISCARD/ : { *(.xt.prop .xt.prop.* .xt.lit .xt.lit.* .xtensa.info .comment .data .data.* .bss .bss.*) }
}

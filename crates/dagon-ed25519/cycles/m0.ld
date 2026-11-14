/* The program starts at `run`, its code in flash at the RP2040's XIP base,
   its data in RAM: the emulator loads the segments where they link. */
ENTRY(run)
MEMORY
{
  FLASH : ORIGIN = 0x10000000, LENGTH = 1M
  RAM   : ORIGIN = 0x20000000, LENGTH = 256K
}
SECTIONS
{
  .text : { *(.text .text.*) } > FLASH
  .rodata : { *(.rodata .rodata.*) } > FLASH
  .data : { *(.data .data.*) } > RAM
  .bss : { *(.bss .bss.*) *(COMMON) } > RAM
  /DISCARD/ : { *(.ARM.exidx .ARM.exidx.*) }
}

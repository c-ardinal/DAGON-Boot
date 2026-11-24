/* ESP32-S3 boot core: RAM only, loaded by stage0 into what an app may use
 * (below stage0's window, dagon_mcu_esp32s3::app_image), so nothing runs
 * from the flash it writes. Code at IRAM 0x40380000 (SRAM1, which the data
 * bus sees at 0x3FC90000..0x3FCB0000), data from DRAM 0x3FCB0000, apart on
 * both buses. The stack is the ROM's PRO CPU stack, as stage0's. */
MEMORY
{
  iram (RX) : ORIGIN = 0x40380000, LENGTH = 0x20000
  dram (RW) : ORIGIN = 0x3FCB0000, LENGTH = 0x10000
}
ENTRY(core_start)

SECTIONS
{
  .text : ALIGN(4)
  {
    KEEP(*(.text.core_start))
    *(.literal .text .literal.* .text.*)
    . = ALIGN(4);
  } > iram

  .rodata : ALIGN(4)
  {
    *(.rodata .rodata.* .srodata .srodata.*)
    . = ALIGN(4);
  } > dram

  .data : ALIGN(4)
  {
    *(.data .data.* .sdata .sdata.*)
    . = ALIGN(4);
  } > dram

  .bss (NOLOAD) : ALIGN(4)
  {
    _bss_start = .;
    *(.bss .bss.* .sbss .sbss.* COMMON)
    . = ALIGN(4);
    _bss_end = .;
  } > dram

  /DISCARD/ : { *(.xt.prop .xt.prop.* .xt.lit .xt.lit.* .xtensa.info .comment) }
}

/* The ESP32-S3 ROM's functions (esp-rom-sys 0.1.5 ld/esp32s3/rom, MIT OR
 * Apache-2.0): esp32s3.rom.ld, esp32s3.rom.api.ld. The flash driver and
 * the SHA functions are stage0's (../stage0/src/rom.rs). */
ets_efuse_get_spiconfig = 0x40001f74;
esp_rom_spiflash_config_param = 0x40000a50;
esp_rom_spiflash_read = 0x40000a20;
esp_rom_spiflash_write = 0x40000a14;
esp_rom_spiflash_erase_sector = 0x400009fc;
esp_rom_spiflash_unlock = 0x40000a2c;
spi_flash_attach = 0x40000aec;
software_reset = 0x400006d8;
ets_get_cpu_frequency = 0x40001a40;
ets_sha_enable = 0x40001cec;
ets_sha_disable = 0x40001cf8;
ets_sha_init = 0x40001d10;
ets_sha_starts = 0x40001d28;
ets_sha_update = 0x40001d34;
ets_sha_finish = 0x40001d40;

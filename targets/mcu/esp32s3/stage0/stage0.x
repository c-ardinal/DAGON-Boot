/* ESP32-S3 stage0: the image the ROM loads from flash 0x0 in place of the
 * ESP-IDF 2nd stage. It lives in the window the IDF 2nd stage takes and an
 * esp-hal app leaves alone until it runs (esp-bootloader-esp-idf memory.x:
 * DRAM below 0x3FCDB700 and IRAM below 0x403CB700 are the app's; from
 * 0x3FCE9700 up are the ROM's stack and data). IRAM 0x403CB700 is DRAM
 * 0x3FCDB700 on the other bus, so code and data share that one 56 KiB
 * window: 40 KiB of code, 16 KiB of data. The stack is the ROM's PRO CPU
 * stack. src/load.rs refuses an app segment in this window
 * (dagon_mcu_esp32s3::app_image IRAM and DRAM end where it starts). */
MEMORY
{
  iram (RWX) : ORIGIN = 0x403CB700, LENGTH = 0xA000   /* = DRAM 0x3FCDB700..0x3FCE5700 */
  dram (RW)  : ORIGIN = 0x3FCE5700, LENGTH = 0x4000   /* ..0x3FCE9700 */
}
ENTRY(stage0_start)

SECTIONS
{
  .text : ALIGN(4)
  {
    KEEP(*(.text.stage0_start))
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
 * Apache-2.0): esp32s3.rom.ld, esp32s3.rom.api.ld. */
ets_efuse_get_spiconfig = 0x40001f74;
esp_rom_spiflash_config_param = 0x40000a50;
esp_rom_spiflash_read = 0x40000a20;
esp_rom_spiflash_write = 0x40000a14;
esp_rom_spiflash_erase_sector = 0x400009fc;
esp_rom_spiflash_unlock = 0x40000a2c;
spi_flash_attach = 0x40000aec;
software_reset = 0x400006d8;
Cache_Invalidate_ICache_All = 0x400016d4;
Cache_Invalidate_DCache_All = 0x400016e0;
rom_Cache_Suspend_ICache = 0x4000189c;
Cache_Resume_ICache = 0x400018a8;
rom_Cache_Suspend_DCache = 0x400018b4;
Cache_Resume_DCache = 0x400018c0;
Cache_MMU_Init = 0x40001998;
Cache_Ibus_MMU_Set = 0x400019a4;
Cache_Dbus_MMU_Set = 0x400019b0;
ets_sha_enable = 0x40001cec;
ets_sha_disable = 0x40001cf8;
ets_sha_init = 0x40001d10;
ets_sha_starts = 0x40001d28;
ets_sha_update = 0x40001d34;
ets_sha_finish = 0x40001d40;
/* The measuring build's (feature `measure`). */
ets_get_cpu_frequency = 0x40001a40;
rtc_get_reset_reason = 0x4000057c;

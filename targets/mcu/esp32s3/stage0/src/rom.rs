//! The ROM's functions stage0 calls (addresses in `stage0.x`; prototypes
//! as ESP-IDF's `components/esp_rom/esp32s3/include/esp32s3/rom/*.h`).

extern "C" {
    pub fn ets_efuse_get_spiconfig() -> u32;
    pub fn esp_rom_spiflash_config_param(
        id: u32,
        size: u32,
        block: u32,
        sector: u32,
        page: u32,
        mask: u32,
    ) -> i32;
    pub fn spi_flash_attach(spiconfig: u32, legacy: bool);
    pub fn esp_rom_spiflash_unlock() -> i32;
    pub fn esp_rom_spiflash_read(src: u32, dst: *mut u32, len: u32) -> i32;
    pub fn esp_rom_spiflash_write(dst: u32, src: *const u32, len: u32) -> i32;
    pub fn esp_rom_spiflash_erase_sector(sector: u32) -> i32;
    pub fn software_reset() -> !;

    pub fn rom_Cache_Suspend_ICache() -> u32;
    pub fn Cache_Resume_ICache(autoload: u32);
    pub fn rom_Cache_Suspend_DCache() -> u32;
    pub fn Cache_Resume_DCache(autoload: u32);
    pub fn Cache_Invalidate_ICache_All();
    pub fn Cache_Invalidate_DCache_All();
    pub fn Cache_MMU_Init();
    pub fn Cache_Ibus_MMU_Set(
        ext: u32,
        vaddr: u32,
        paddr: u32,
        psize: u32,
        num: u32,
        fixed: u32,
    ) -> i32;
    pub fn Cache_Dbus_MMU_Set(
        ext: u32,
        vaddr: u32,
        paddr: u32,
        psize: u32,
        num: u32,
        fixed: u32,
    ) -> i32;

    pub fn ets_sha_enable();
    pub fn ets_sha_disable();
    pub fn ets_sha_init(ctx: *mut u32, ty: u32) -> i32;
    pub fn ets_sha_starts(ctx: *mut u32, sha512_t: u16) -> i32;
    pub fn ets_sha_update(ctx: *mut u32, input: *const u8, len: u32, update_ctx: bool);
    pub fn ets_sha_finish(ctx: *mut u32, out: *mut u8) -> i32;
}

/// Writes the peripheral register at `addr`.
pub fn reg_write(addr: u32, v: u32) {
    // SAFETY: a register address of the ESP32-S3's memory map.
    unsafe { core::ptr::write_volatile(addr as *mut u32, v) }
}

/// Reads the peripheral register at `addr`.
pub fn reg_read(addr: u32) -> u32 {
    // SAFETY: a register address of the ESP32-S3's memory map.
    unsafe { core::ptr::read_volatile(addr as *const u32) }
}

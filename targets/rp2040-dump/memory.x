/* SRAM only. ENTRY holds the trampoline the bootrom calls; "FLASH" is the
   code area (also in SRAM, cortex-m-rt's name for it); RAM holds data and
   the stack. */
MEMORY {
    ENTRY : ORIGIN = 0x20000000, LENGTH = 0x100
    FLASH : ORIGIN = 0x20000100, LENGTH = 0x2FF00
    RAM   : ORIGIN = 0x20030000, LENGTH = 0x10000
}

SECTIONS {
    .entry ORIGIN(ENTRY) :
    {
        KEEP(*(.entry));
    } > ENTRY
}

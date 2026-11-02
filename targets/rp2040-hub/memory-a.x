/* Hub app slot A body: after the 512-byte inline header. */
MEMORY {
    FLASH : ORIGIN = 0x10048200, LENGTH = 1024K - 0x200
    RAM   : ORIGIN = 0x20000000, LENGTH = 256K
}

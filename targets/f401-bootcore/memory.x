/* Boot core: sectors 0-1. The last 512 bytes (0x08007E00) hold its signed
   trailer header, so code must end before them. */
MEMORY
{
  FLASH : ORIGIN = 0x08000000, LENGTH = 0x7E00
  RAM   : ORIGIN = 0x20000000, LENGTH = 96K
}

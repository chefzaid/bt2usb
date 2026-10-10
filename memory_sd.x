/* Linker script for nRF52840 with SoftDevice S140 v7.3.0
 *
 * The SoftDevice occupies the first 0x27000 bytes of flash and
 * the first 0x6000 bytes of RAM. Application code and data
 * start after those regions.
 *
 * nRF52840 totals:
 *   Flash: 1024 KB (0x0010_0000)
 *   RAM:     256 KB (0x0004_0000)
 */

MEMORY
{
    /*
     * Flash: starts after SoftDevice (0x0002_7000) and stops at the
     * paired-device/bond storage region (config::STORAGE_FLASH_START =
     * page 240 = 0x000F_0000, 4 pages). Keeping storage outside FLASH means
     * the linker errors out instead of silently placing code or rodata on
     * pages that `sequential-storage` erases at runtime. The assertion below
     * fails the link if this length and config.rs disagree.
     * Length: 0xF0000 - 0x27000 = 804K. (0xF4000..0x100000 is left unused.)
     */
    FLASH : ORIGIN = 0x00027000, LENGTH = 804K

    /*
     * RAM: starts after SoftDevice RAM reservation (0x2000_6000)
     * Length: 256K - 24K (SoftDevice) = 232K
     *
     * NOTE: If you get SoftDevice RAM errors at runtime, increase the
     * origin here and decrease the length accordingly. The SoftDevice
     * RAM requirement depends on the number of connections, MTU size,
     * and enabled features. Current reservation (24 KB) is generous
     * for 2 central connections with MTU 64.
     */
    RAM : ORIGIN = 0x20006000, LENGTH = 232K
}

/* build.rs defines __bt2usb_storage_start and __bt2usb_storage_end from
 * STORAGE_FLASH_START and STORAGE_FLASH_END in src/config.rs, ahead of this
 * file. FLASH must end exactly where pairing storage starts: ending later puts
 * code on pages the store erases, and ending earlier means the two files no
 * longer describe the same layout. */
ASSERT(ORIGIN(FLASH) + LENGTH(FLASH) == __bt2usb_storage_start,
       "FLASH in memory_sd.x must end at STORAGE_FLASH_START in src/config.rs; change both together");
ASSERT(__bt2usb_storage_end <= 0x00100000,
       "pairing storage (STORAGE_FLASH_PAGE_START/COUNT in src/config.rs) must end within the 1 MB of flash");

/* The application RAM base that nrf-softdevice hands to the SoftDevice
 * (APP_RAM_BASE) is `__sdata`, so .data must start right at ORIGIN(RAM) with
 * the stack at the top of RAM. A linker that moves the stack below .data
 * (e.g. flip-link, which also rewrites ORIGIN(RAM)) would make the SoftDevice
 * claim and write-protect everything underneath, including the stack. */
ASSERT(__sdata == ORIGIN(RAM) && _stack_start == ORIGIN(RAM) + LENGTH(RAM),
       "stack must sit at the top of RAM and .data at ORIGIN(RAM): the SoftDevice uses __sdata as APP_RAM_BASE (is flip-link in use?)");

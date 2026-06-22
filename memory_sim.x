/* Linker script for the SoftDevice-free `sim` build (Renode / Layer 3).
 *
 * Without the Nordic SoftDevice there is no reserved flash/RAM region, so the
 * application owns the whole device and links from the start of flash/RAM.
 *
 * nRF52840 totals:
 *   Flash: 1024 KB (0x0010_0000)
 *   RAM:     256 KB (0x0004_0000)
 */

MEMORY
{
    FLASH : ORIGIN = 0x00000000, LENGTH = 1024K
    RAM   : ORIGIN = 0x20000000, LENGTH = 256K
}

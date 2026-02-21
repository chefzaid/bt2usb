/* Linker script for nRF52840 with SoftDevice S140 v7.3.0
 *
 * The SoftDevice occupies the first 0x27000 bytes of flash and
 * the first 0x20000 bytes of RAM. Application code and data
 * start after those regions.
 *
 * nRF52840 totals:
 *   Flash: 1024 KB (0x0010_0000)
 *   RAM:     256 KB (0x0004_0000)
 */

MEMORY
{
    /*
     * Flash: starts after SoftDevice (0x0002_7000)
     * Length: 1024K - 156K (SoftDevice) = 868K
     */
    FLASH : ORIGIN = 0x00027000, LENGTH = 868K

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

# Hardware And Configuration Reference

The implemented board target is the nRF52840 with native USB, using the DK pin
mapping below. Other boards require a review of their pin routing, clocks, power
supplies, and flash layout. There are no implemented ESP32, RP2040, or STM32 ports.

## Parts And Wiring

| Component | Development example | Purpose |
| --- | --- | --- |
| MCU board | nRF52840-DK | BLE radio, native USB, internal flash, debug probe |
| Display | SSD1306 128×64 I2C module | Pairing menu and connection status |
| Buttons | Three active-low switches | UP, DOWN, SELECT; DK buttons may be used |
| Cables | USB data cables and jumpers | Native USB to host, debugger to development PC |
| Enclosure | Board-specific enclosure | Protection and mounting |

| Signal | Pin | Connection |
| --- | --- | --- |
| UP | P0.11 | Switch to GND, internal pull-up |
| DOWN | P0.12 | Switch to GND, internal pull-up |
| SELECT | P0.24 | Switch to GND, internal pull-up |
| OLED SDA | P0.26 | SSD1306 SDA |
| OLED SCL | P0.27 | SSD1306 SCL |
| OLED power | VDD / GND | 3.3 V / common GND |
| USB D+/D− | Native USB connector | Routed on the DK; no jumper wiring |

The firmware's pin assignments are instantiated in [main.rs](../src/main.rs),
[selftest.rs](../src/selftest.rs), and [sim.rs](../src/sim.rs). The comments in
[config.rs](../src/config.rs) document the defaults; changing comments alone does
not reroute peripherals. P0.06 is listed as an unused status LED in config.

During bring-up, connect native USB directly to the PC. Then move it to the
monitor hub and connect the monitor's USB upstream cable to the PC. A USB
extension or USB-C/USB-A adapter may make the controls easier to reach.

## Configuration Defaults

These are compile-time settings from [config.rs](../src/config.rs).

| Setting | Default | Meaning |
| --- | --- | --- |
| `BLE_SCAN_DURATION_SECS` | 8 | Scan window in seconds |
| `BLE_MAX_DISCOVERED` | 8 | Maximum cached scan results |
| `BLE_CONN_INTERVAL_MIN` / `MAX` | 6 / 12 | 7.5–15 ms, in 1.25 ms units |
| `BLE_SUP_TIMEOUT` | 400 | 4-second supervision timeout, in 10 ms units |
| `BLE_CONNECT_TIMEOUT_SECS` | 6 | Connection-attempt / bonded-peer resolution scan timeout |
| `BLE_RECONNECT_BACKOFF_MS` | 500 | Pause between reconnect attempts |
| `MAX_PAIRED_DEVICES` | 4 | Stored peers; active slots are separately limited to two |
| `STORAGE_FLASH_PAGE_START` / `COUNT` | 240 / 4 | Pairing storage reservation |
| `USB_VID` / `USB_PID` | `0x1209` / `0x0001` | Development IDs; obtain an assigned production identity |
| `USB_HID_POLL_MS` | 1 | USB interrupt endpoint polling interval |
| `BUTTON_DEBOUNCE_MS` | 50 | Button debounce interval |
| `SCREEN_AUTO_OFF_ENABLED` | `true` | Enable OLED inactivity power-off |
| `SCREEN_AUTO_OFF_TIMEOUT_SECS` | 120 | OLED inactivity timeout |

The USB serial is generated from the two factory `FICR.DEVICEID` words as a
16-character uppercase hexadecimal value in `usb/hid_device.rs`. It is stable
across firmware updates and USB ports; there is no shared serial constant.

## Memory Layout

The nRF52840 has 1 MiB internal flash and 256 KiB RAM. The configured reservations
in [memory_sd.x](../memory_sd.x) are:

| Region | Address range, end exclusive | Size |
| --- | --- | --- |
| SoftDevice flash | `0x00000000–0x00027000` | 156 KiB |
| Application flash | `0x00027000–0x000F0000` | 804 KiB |
| Pairing/bond storage | `0x000F0000–0x000F4000` | 16 KiB |
| Unused tail flash | `0x000F4000–0x00100000` | 48 KiB |
| SoftDevice RAM reservation | `0x20000000–0x20006000` | 24 KiB |
| Application RAM | `0x20006000–0x20040000` | 232 KiB |

These are reservations, not measured firmware usage. Measure release size with
`mask size` and stack high-water on the board. Record the RAM requirement printed
when SoftDevice is enabled before changing its reservation. No bootloader or DFU
region is currently allocated.

The linker excludes the pairing pages from application flash; their contents
are defined in the [data model](data-model.md#pairing-store). Keep the storage
constants and linker map consistent when changing either. SoftDevice uses
`__sdata` as its application RAM boundary, so `.data` must begin at the RAM origin
and the stack must remain at the top; the linker asserts this relationship.

## Possible Future Ports

ESP32-S3, RP2040 with an external BLE module, and STM32 with an external BLE module
were previously listed as alternatives. They remain design ideas requiring
separate HAL, BLE, USB, pin, memory, and validation work. Only the nRF52840 build
is maintained here.

## Related Guides

- [First flash](first-flash.md)
- [Architecture and ADRs](architecture.md)
- [Data model](data-model.md)
- [Development](development.md)

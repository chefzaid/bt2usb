# Hardware And Configuration Reference

The implemented board target is the nRF52840 with native USB, using the DK pin
mapping below. Other boards require a review of their pin routing, clocks, power
supplies, and flash layout. There are no implemented ESP32, RP2040, or STM32 ports.

This guide is the reference for what the firmware expects from the hardware:
parts, wiring, electrical behavior of the buttons and display, power, clocks
and radio settings, compile-time configuration, the memory map, and what to
change for another nRF52840 board. Bring-up steps are in
[first flash](first-flash.md); the byte formats stored in flash are in the
[data model](data-model.md).

No hardware acceptance record is committed yet (see the validation record in
[testing](testing.md)). Statements below about pins, clocks and memory come
from source and linker scripts; statements about how a particular board,
display, hub or host behaves are acceptance targets until a recorded
first-flash run covers them.

## Parts And Wiring

### Bill Of Materials

| Item | Quantity | Development example | Notes |
| --- | --- | --- | --- |
| MCU board | 1 | nRF52840-DK | BLE radio, native USB, internal flash. The board must route the nRF52840's USB D+, D− and VBUS to a connector and expose SWD |
| Debug probe | 1 | The DK's on-board debugger, reached through its debugger USB port (J2) | Other boards need an SWD probe that `probe-rs` supports (`mask probe-list`) |
| SoftDevice | 1 image | Nordic S140 v7.3.0 (`s140_nrf52_7.3.0_softdevice.hex`) | Flashed once per board with `mask softdevice`; the memory map assumes this version |
| Display | 1 | SSD1306 128×64 I2C module at address `0x3C` | A module strapped to `0x3D` needs a code change; see [OLED Display](#oled-display) |
| Buttons | 3 | DK buttons 1–3, or normally open momentary switches | Active-low to GND; no external resistors needed |
| USB cables | 2 | Data-capable cables | One for the debugger port, one for the nRF USB port (J3) that the PC sees; a charge-only cable fails enumeration |
| Jumper wires | As needed | Female-to-female or to the DK headers | OLED SDA, SCL, VDD, GND; external buttons if used |
| BLE peripherals | Up to 2 active, 4 stored | BLE HID-over-GATT keyboard and mouse | Bluetooth Classic devices are not supported |
| Host side | 1 each | PC; monitor with a USB hub | The hub's upstream cable goes to the PC; hub power behavior affects the bridge (see [Power](#power)) |
| Enclosure | Optional | Board-specific | No enclosure design is maintained in this repository |

### Wiring

| Signal | Pin | Connection |
| --- | --- | --- |
| UP | P0.11 | Switch to GND, internal pull-up |
| DOWN | P0.12 | Switch to GND, internal pull-up |
| SELECT | P0.24 | Switch to GND, internal pull-up |
| OLED SDA | P0.26 | SSD1306 SDA |
| OLED SCL | P0.27 | SSD1306 SCL |
| OLED power | VDD / GND | The board's I/O supply rail and common ground; not a 5 V rail |
| USB D+/D− | Native USB connector | Routed on the DK; no jumper wiring |

The firmware's pin assignments are instantiated in [main.rs](../src/main.rs),
[selftest.rs](../src/selftest.rs), and [sim.rs](../src/sim.rs). The comments in
[config.rs](../src/config.rs) document the defaults; changing comments alone does
not reroute peripherals. The `config.rs` comment also lists P0.06 as a status
LED, which no code drives (see below).

On the DK, P0.11, P0.12 and P0.24 are also wired to on-board buttons 1, 2 and
3, so those work as UP, DOWN and SELECT without extra switches. P0.26 and P0.27
are free GPIOs on the DK. Power the OLED from VDD so any pull-ups on the module
pull SDA and SCL to the nRF52840's I/O voltage; nRF52840 GPIOs are not 5 V
tolerant.

During bring-up, connect native USB directly to the PC. Then move it to the
monitor hub and connect the monitor's USB upstream cable to the PC. A USB
extension or USB-C/USB-A adapter may make the controls easier to reach.

### Pin And Peripheral Usage

| Resource | Bridge (`bt2usb`) | Self-test | Simulation (`bt2usb-sim`) |
| --- | --- | --- | --- |
| P0.11, P0.12, P0.24 | Buttons, GPIO input with pull-up | Same, with an idle-level check | Same, driven by Renode |
| P0.26, P0.27 | TWIM0 SDA, SCL | Same | Not used |
| P0.06, P0.08 | Not used | Not used | UARTE0 TX and RX for the log |
| P0.18 | Pin reset, set in UICR by `embassy_nrf::init` | Same | Same `init` code; Renode's UICR handling not checked |
| P0.09, P0.10 | NFC antenna pins; UICR `NFCPINS` left in NFC mode | Same | Same `init` code; not used |
| USBD | Composite HID device | Same device, one idle mouse report | Not used |
| TWISPI0 (TWIM0) | OLED | OLED probe and Home screen | Not used |
| GPIOTE | Async button edges | Async button waits | Async button edges |
| RTC1 | `embassy-time` driver (`time-driver-rtc1`) | Same | Same |
| Internal flash pages 240–243 | Pairing store | Scratch record, then removed | Not used |

The SoftDevice owns the radio and the POWER and CLOCK peripherals. Because it
owns POWER, the application learns about USB power through SoftDevice events
(comments in [main.rs](../src/main.rs) and
[usb/hid_device.rs](../src/usb/hid_device.rs)).
Flash writes go through the SoftDevice flash API. Check Nordic's S140
specification before claiming any further peripheral, timer or PPI resource.

P0.18, P0.09 and P0.10 are not free GPIOs. The bridge and the self-test pass
`embassy_nrf::init` its default configuration with only the interrupt
priorities changed, and [Cargo.toml](../Cargo.toml) enables neither the
`reset-pin-as-gpio` nor the `nfc-pins-as-gpio` feature of `embassy-nrf`. On
every boot `init` therefore makes sure UICR `PSELRESET` selects P0.18 as the
pin reset and `NFCPINS` keeps P0.09 and P0.10 in NFC mode. On chips with build
code `F` or later it also writes the UICR value that keeps the SWD port open.
Whenever it had to write UICR, it resets the chip once. The simulation build
uses the same default configuration. This is source-derived from `embassy-nrf`
0.7.0, not observed on a board; the UICR behavior and its security effect are
in [security](security.md#physical-access-and-debug-port).

`config.rs` names P0.06 as a status LED, but no firmware code drives it. On the
nRF52840-DK the user LEDs are on P0.13–P0.16 and P0.06 is the UART TX line to
the on-board debugger's virtual serial port; the simulation build uses the same
pin for its UART log.

The SoftDevice reserves interrupt priorities 0, 1 and 4. The bridge and the
self-test set GPIOTE, the time driver, USBD and TWISPI0 to priority 2
([main.rs](../src/main.rs)). Any new peripheral interrupt must use 2, 3, 5, 6
or 7. The simulation build has no SoftDevice and keeps the `embassy-nrf`
defaults.

## Buttons

| Property | Value | Source |
| --- | --- | --- |
| Electrical | Active-low; pressed connects the pin to GND | [ui/buttons.rs](../src/ui/buttons.rs) |
| Pull | nRF52840 internal pull-up (`Pull::Up`); no external resistor | `button_task` |
| Debounce | 50 ms (`BUTTON_DEBOUNCE_MS`) on press and on release | [config.rs](../src/config.rs) |
| Events | One `ButtonEvent` per press; no auto-repeat or long press | `button_task` |
| Queue | `BUTTON_CHANNEL`, capacity 4; the task waits when it is full | [main.rs](../src/main.rs) |

Each button has its own task. It waits for a low level, waits 50 ms, and sends
the event only if the pin is still low. It then waits for a high level that is
still high 50 ms later before it re-arms. Consequences:

- A press shorter than the debounce time can be missed.
- A button held at boot produces one event once the task starts.
- A release that happens while the task waits on a full channel is still seen,
  because the task waits for a level, not an edge.

The UI decides what a press does. While the OLED is off, the first press only
wakes it; while a saved-device request is pending, presses are ignored
([data model](data-model.md#ui-state-model)).

In Renode, button edges are injected on `gpio0` and reach the same driver
through custom GPIO and GPIOTE models of the pin SENSE, LATCH and DETECT
chain that raises the GPIOTE PORT event
([ADR 0014](adr/0014-renode-gpio-models.md), [testing](testing.md#renode-simulation)).
The self-test checks each pin reads high at rest, then waits up to 20 s for a
press.

## OLED Display

| Property | Value | Source |
| --- | --- | --- |
| Controller and size | SSD1306, 128×64, rotation 0 | [ui/display.rs](../src/ui/display.rs) |
| Bus | TWIM0 (`TWISPI0`), SDA P0.26, SCL P0.27 | [main.rs](../src/main.rs) |
| I2C address | `0x3C`, from the `ssd1306` crate's default `I2CDisplayInterface::new`; the self-test probes the same `OLED_ADDR` | [ui/display.rs](../src/ui/display.rs), [selftest.rs](../src/selftest.rs) |
| Bus frequency | Not set; the `embassy-nrf` `twim::Config` default applies | [main.rs](../src/main.rs) |
| Pull-ups | Internal SDA and SCL pull-ups enabled (about 13 kΩ per the source comment); harmless alongside module pull-ups | [main.rs](../src/main.rs) |
| Transmit scratch buffer | 64 bytes in RAM, needed for flash-resident command sequences | [main.rs](../src/main.rs) |
| Rendering | Buffered graphics mode (a 1024-byte framebuffer for 128×64 pixels), `FONT_6X10` | [ui/display.rs](../src/ui/display.rs) |
| Display power | `set_display_on(false)` after inactivity; the module stays powered | [ui/display.rs](../src/ui/display.rs) |
| Operation deadline | 500 ms, then a TWIM STOP request while the DMA future is kept | `finish_or_stop` |
| Retry backoff | 1 s, doubling, capped at 30 s, reset after a success | [ui/display_logic.rs](../src/ui/display_logic.rs) |

The display runs in its own task and only ever renders the latest published
frame, so an I2C fault cannot stall input delivery
([ADR 0009](adr/0009-isolated-display-task.md)). On a NACK or overrun the
`StopSafeI2c` wrapper waits for the TWIM STOPPED event before returning, because
the pinned driver returns before the stop completes. A bus held low
electrically leaves the display task degraded while the bridge continues; the
log then repeats `"OLED I2C error: STOP not complete; requesting again"`, or
shows `"OLED I2C stalled; requesting STOP, display task degraded until DMA completes"`
after the 500 ms deadline. Normal recovery logs `"OLED initialized/recovered"`;
a failed attempt logs
`"OLED operation failed; retry in {} ms (bridge remains active)"`.

For a module at `0x3D`, construct the display interface with the `ssd1306`
crate's alternate-address constructor in `ui/display.rs` and change `OLED_ADDR`
in `selftest.rs`. The self-test's first write is the command stream
`[0x00, 0xAE]` (display off), which a missing panel NACKs.

## USB Connection

| Property | Value | Source |
| --- | --- | --- |
| Connector | The nRF USB port (J3 on the DK), not the debugger port | [first flash](first-flash.md) |
| VBUS detection | Software detector fed by SoftDevice USB power events | [sd_setup.rs](../src/sd_setup.rs), [main.rs](../src/main.rs) |
| Boot state | Read from the SoftDevice's USB regulator status: bit 0 VBUS detected, bit 1 output ready (`"USB power: vbus={} ready={}"`) | `enable_usb_power_events` |
| Fallback | If the events cannot be enabled, VBUS is assumed present (`"failed to enable USB power events; assuming VBUS present"`); if the regulator status cannot be read, VBUS is also assumed present, without a log line | `enable_usb_power_events` |
| Identity and descriptors | VID/PID, strings, serial, interfaces | [data model](data-model.md#usb-device-identity) |

The SoftDevice owns the POWER peripheral, so the application cannot use the
hardware VBUS detector. `softdevice_task` forwards `PowerUsbDetected`,
`PowerUsbRemoved` and `PowerUsbPowerReady` to the USB driver, which is how an
unplug and replug is noticed after boot. The self-test prints
`"USB: no VBUS yet; plug the nRF USB port (not the debugger port) into the PC"`
when it sees no VBUS.

## Power

The bridge is designed to be bus-powered from the PC or the monitor's USB hub
and to stay connected rather than sleep
([ADR 0012](adr/0012-bus-powered-no-system-off.md)).

| Aspect | Implemented behavior | Source |
| --- | --- | --- |
| Supply | USB bus power; there is no battery support or battery measurement | [power.rs](../src/power.rs) |
| Deep sleep | Never enters System-OFF, which would drop USB enumeration and the BLE links | [power.rs](../src/power.rs) |
| CPU idle | The Embassy executor waits for events between tasks | [power.rs](../src/power.rs) |
| BLE timing | 7.5–15 ms connection interval, latency 0, in every power state | [config.rs](../src/config.rs) |
| Power states | `Active`; `Idle` after 60 s without activity; `LowPower` while USB is suspended, or after 120 s without activity and with no BLE link | [power_logic.rs](../src/power_logic.rs), `IDLE_TIMEOUT_SECS` in [power.rs](../src/power.rs) |
| What the states change | Only whether the OLED is on; `LowPower` turns it off | `PowerManager::display_on` |
| OLED auto-off | After 120 s without activity (the sources are listed below) | `SCREEN_AUTO_OFF_TIMEOUT_SECS` |
| Declared USB power | 100 mA in the configuration descriptor | [usb/hid_device.rs](../src/usb/hid_device.rs) |
| Measured current | Not measured; no figure is recorded in this repository | — |

The 100 mA declaration is a budget the host or hub may enforce, not a measured
value. Measure the board plus OLED with two links active, during scans, and
with the display on before relying on it. The firmware does nothing to reduce
current while the host suspends the bus: BLE links, reconnect attempts and the
radio stay active, and no attempt is made to meet the USB suspend-current
limit. Whether a given host or hub tolerates that is a hardware acceptance
question.

Activity that keeps the display on and the state `Active` comes from button
presses, every `BleEvent::Connected` (including the one sent when one of two
links drops and the other stays up), USB resume, and HID reports passing
through the dispatcher (`note_hid_activity`). USB suspend forces `LowPower`
immediately and resume returns to `Active`.

Hub and DK considerations:

- Some monitors switch their hub off with the panel. The bridge then loses
  power and boots again when power returns, reconnecting up to two stored peers.
  Losing power during a flash write is not yet a tested case; power-loss-safe
  persistence is open in [TODO.md](../TODO.md).
- The DK can be powered from either USB connector. During bring-up both are
  usually connected; in the monitor only the nRF USB cable is. Set the DK's
  power switch and power-source selection, as described in Nordic's DK
  documentation for your board revision, so the board runs from the nRF USB
  connector alone, and confirm it does before moving it to the hub. The
  firmware cannot detect how the board is powered.
- Record the supply arrangement with the hardware acceptance evidence, as
  [testing](testing.md#hardware-acceptance-evidence) requires.

## Clocks And Radio

The SoftDevice configuration is shared by the bridge and the self-test in
[sd_setup.rs](../src/sd_setup.rs)
([ADR 0002](adr/0002-nrf52840-softdevice-embassy.md)).

| Setting | Value | Meaning |
| --- | --- | --- |
| Low-frequency clock source | Internal RC oscillator (`NRF_CLOCK_LF_SRC_RC`) | No 32.768 kHz crystal is required or used |
| RC calibration interval | `rc_ctiv = 16` | In SoftDevice units of 0.25 s: every 4 s |
| Temperature-based calibration | `rc_temp_ctiv = 2` | Calibrate when the temperature changed, and at least every second interval |
| Declared LF accuracy | 500 ppm | Used by the SoftDevice for timing windows |
| High-frequency crystal | Not requested by the application | The SoftDevice manages it around radio activity |
| Link count | `conn_count = 2`, `central_role_count = 2`, `central_sec_count = 2` | Two central links, both able to use security |
| Advertising and peripheral role | `adv_set_count = 0`, `periph_role_count = 0` | The bridge never advertises |
| Connection event length | 6 (`BLE_CONN_EVENT_LENGTH`), 7.5 ms | Short enough for two links to interleave |
| ATT MTU | 64 bytes, requested on each connection by `connect_with_security` | Bounds GATT fragments; Report Maps are read in pieces |
| GAP device name and other options | Not set | SoftDevice defaults |

The values are from `sd_setup.rs`; the meaning of the clock fields (units of
`rc_ctiv` and `rc_temp_ctiv`) comes from Nordic's S140 API documentation
(`nrf_sdm.h`), which is not in this repository.

The nRF52840-DK fits a 32.768 kHz crystal, but this firmware does not use it.
A board with a crystal may switch to `NRF_CLOCK_LF_SRC_XTAL` with the crystal's
accuracy and both calibration fields set to 0; that change needs its own
hardware test. The application never starts the high-frequency crystal
oscillator itself (there is no `sd_clock_hfclk_request` call). How the USB
peripheral's clock requirement is met under this arrangement has not been
verified on hardware; check USB stability during bring-up.

Radio parameters that are not in `config.rs` come from the vendored
`nrf-softdevice` defaults ([ScanConfig in central.rs](../vendor/nrf-softdevice/src/ble/central.rs)):

| Parameter | Value |
| --- | --- |
| Scan type | Active for user scans, so scan responses supply names; passive for reconnect scans, which need only the advertiser's address |
| Extended scanning | Enabled (vendored default) |
| PHY | 1M |
| Scan interval / window | User scans and slow reconnect scans: the vendored default of 2732 / 500 in 0.625 ms units, about 1.7 s / 312.5 ms. Reconnect scans for 30 s after power-up or a lost link, and every connection attempt: 160 / 80, a 50 ms window every 100 ms |
| Scan and initiator TX power | 0 dBm (`TxPower::ZerodBm`); connections inherit it |
| User scan duration | Stops at the first advertisement after 8 s (`BLE_SCAN_DURATION_SECS`), or at a 10 s backstop; there is no boot scan |
| Connection attempt | Whitelist scan for the target at 160 / 80, timeout 6 s (`BLE_CONNECT_TIMEOUT_SECS`) |
| Reconnect scan | Up to 6 s for an address that either slot's stored IRK resolves or that equals its stored address; a device heard for the other slot is handed over |
| Connection parameters | 7.5–15 ms interval, slave latency 0, 4 s supervision timeout; a peripheral's later request is answered within 7.5–15 ms (up to 30 ms for one that asks only for slower intervals), latency at most 20, and a 1–4 s supervision timeout |
| Security | `IoCapabilities::None` (Just Works), bonding allowed; before HID discovery the worker polls up to 25 times, 200 ms apart (about 5 s), for an encrypted security mode (`wait_for_secure_link` in [multi_conn.rs](../src/ble/multi_conn.rs)) |

One GAP scan or connection setup runs at a time (`GAP_PROCEDURE` in
[ble/mod.rs](../src/ble/mod.rs)), because the SoftDevice rejects a second one.
Pairing is unauthenticated Just Works for now
([ADR 0011](adr/0011-interim-just-works-pairing.md),
[security](security.md#pairing-and-authentication)). No antenna, range or RF
coexistence measurements are recorded.

## Configuration Defaults

These are compile-time settings from [config.rs](../src/config.rs).

| Setting | Default | Meaning |
| --- | --- | --- |
| `BLE_SCAN_DURATION_SECS` | 8 | Scan window in seconds |
| `BLE_MAX_DISCOVERED` | 8 | Maximum cached scan results |
| `BLE_CONN_INTERVAL_MIN` / `MAX` | 6 / 12 | 7.5–15 ms, in 1.25 ms units |
| `BLE_SLAVE_LATENCY` | 0 | Connection events a peripheral may skip |
| `BLE_CONN_EVENT_LENGTH` | 6 | 7.5 ms SoftDevice event length, in 1.25 ms units |
| `BLE_SUP_TIMEOUT` | 400 | 4-second supervision timeout, in 10 ms units |
| `BLE_CONNECT_TIMEOUT_SECS` | 6 | Connection-attempt and reconnect scan timeout |
| `BLE_RECONNECT_BACKOFF_MS` | 500 | Pause between reconnect attempts |
| `BLE_FAST_SCAN_INTERVAL` / `WINDOW` | 160 / 80 | A 50 ms window every 100 ms, in 0.625 ms units, for connection attempts and fast reconnect scans |
| `BLE_FAST_RECONNECT_SECS` | 30 | How long reconnect scans stay fast after power-up or a lost link |
| `BLE_FAILED_RECONNECT_HOLDOFF_MS` | 6500 | How long the other slot's reconnect scans ignore a device after a failed attempt; derived from `BLE_CONNECT_TIMEOUT_SECS` and `BLE_RECONNECT_BACKOFF_MS` |
| `BLE_PEER_MAX_CONN_INTERVAL` | 24 | Longest interval (30 ms) granted to a peripheral that asks only for intervals slower than 15 ms |
| `BLE_MAX_PERIPHERAL_LATENCY` | 20 | Largest peripheral latency granted to a peripheral's request |
| `BLE_MIN_SUP_TIMEOUT` | 100 | Shortest supervision timeout granted to a peripheral's request (1 s); the longest is `BLE_SUP_TIMEOUT` |
| `MAX_PAIRED_DEVICES` | 4 | Stored peers; active slots are separately limited to two |
| `STORAGE_FLASH_PAGE_START` / `COUNT` | 240 / 4 | Pairing storage reservation |
| `USB_VID` / `USB_PID` | `0x1209` / `0x0001` | Development IDs; obtain an assigned production identity |
| `USB_MANUFACTURER` / `USB_PRODUCT` | `bt2usb` / `BT-to-USB HID Bridge` | USB string descriptors |
| `USB_HID_POLL_MS` | 1 | USB interrupt endpoint polling interval |
| `BUTTON_DEBOUNCE_MS` | 50 | Button debounce interval |
| `SCREEN_AUTO_OFF_ENABLED` | `true` | Enable OLED inactivity power-off |
| `SCREEN_AUTO_OFF_TIMEOUT_SECS` | 120 | OLED inactivity timeout |

The USB serial is generated from the two factory `FICR.DEVICEID` words as a
16-character uppercase hexadecimal value in `usb/hid_device.rs`. It is stable
across firmware updates and USB ports; there is no shared serial constant.

### Constants Outside config.rs

These hardware-relevant values live next to the code that uses them:

| Value | Default | Source |
| --- | --- | --- |
| Simultaneous BLE links (`MAX_CONNECTIONS`) | 2 | [ble/coordinator.rs](../src/ble/coordinator.rs) |
| Idle timeout (`IDLE_TIMEOUT_SECS`) | 60 s | [power.rs](../src/power.rs) |
| Flash write attempts and retry pause | 3, 20 ms apart | [storage.rs](../src/storage.rs) |
| Pairing item size limit (`MAX_RECORD_SIZE`) | 512 bytes | [storage.rs](../src/storage.rs) |
| USB endpoint write deadline and retry backoff | 100 ms; 20 ms doubling to 1000 ms | [hid/delivery.rs](../src/hid/delivery.rs) |
| Endpoint queue (`ENDPOINT_QUEUE_CAPACITY`) | 16 reports per endpoint | [hid/delivery.rs](../src/hid/delivery.rs) |
| OLED address in the self-test (`OLED_ADDR`) | `0x3C` | [selftest.rs](../src/selftest.rs) |
| OLED operation deadline, retry backoff | 500 ms; 1 s doubling to 30 s | [ui/display.rs](../src/ui/display.rs), [ui/display_logic.rs](../src/ui/display_logic.rs) |
| Self-test USB, button and scan limits | 10 s, 20 s, 8 s | [selftest.rs](../src/selftest.rs) |

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

The memory decisions are recorded in
[ADR 0010](adr/0010-static-memory-layout.md). Further details:

- [build.rs](../build.rs) copies `memory_sd.x` (bridge and self-test) or
  `memory_sim.x` (simulation) to `OUT_DIR/memory.x`. The sources are
  deliberately not named `memory.x`, so a stray root file cannot shadow the
  selected layout, and the build refuses `embedded` and `sim` together.
- The assertion in `memory_sd.x` is
  `__sdata == ORIGIN(RAM) && _stack_start == ORIGIN(RAM) + LENGTH(RAM)`.
  `flip-link` would break it, so [.cargo/config.toml](../.cargo/config.toml)
  links with `link.x`, `defmt.x` and `--nmagic`, without `flip-link`.
- At enable time the vendored `nrf-softdevice` logs `"softdevice RAM: {:?} bytes"`.
  If the reservation is too small it panics with
  `"too little RAM for softdevice. Change your app's RAM start address to {:x}"`;
  if it is larger than needed it warns
  `"You're giving more RAM to the softdevice than needed. You can change your app's RAM start address to {:x}"`.
  The `memory_sd.x` comment calls 24 KiB generous for two central links with a
  64-byte MTU; do not shrink it without a measured value.
- There is no heap. Embassy task futures are statically allocated, and every
  task shares the single stack at the top of RAM. The stack is painted with
  `0xCCCCCCCC` at reset (`cortex-m-rt` `paint-stack`), and the bridge logs
  `"stack high-water: {} of {} bytes"` whenever the deepest use grows. The
  self-test fails its stack stage if half or more of the region was used.

The simulation build has no SoftDevice and owns the whole device:

| Region | Address range, end exclusive | Size |
| --- | --- | --- |
| Flash ([memory_sim.x](../memory_sim.x)) | `0x00000000–0x00100000` | 1024 KiB |
| RAM | `0x20000000–0x20040000` | 256 KiB |

## Porting To Another nRF52840 Board

Work through this list for any board other than the nRF52840-DK, then run the
full [first-flash checklist](first-flash.md). Nothing below has been done for
another board yet.

- [ ] **USB.** Confirm the board routes the nRF52840's USB D+, D− and VBUS to a
      connector the PC or hub can reach. The firmware learns about the cable
      only through VBUS events.
- [ ] **Pins.** Choose GPIOs for the three buttons and the I2C bus that the
      board does not dedicate to something else, for example the 32.768 kHz
      crystal pins (P0.00, P0.01), the NFC antenna pins (P0.09, P0.10), the pin
      reset (P0.18), on-board flash or LEDs. P0.09/P0.10 and P0.18 stay
      reserved unless `embassy-nrf`'s `nfc-pins-as-gpio` or `reset-pin-as-gpio`
      feature is enabled; freeing P0.18 on a chip whose UICR already selects it
      as reset also needs a UICR erase
      ([UICR behavior](security.md#physical-access-and-debug-port)). Change
      them in `main.rs` and `selftest.rs` (including the pin names in the
      self-test messages), update the comments in `config.rs`, and update
      `sim.rs`, the Renode scripts and this guide if the simulation should
      follow.
- [ ] **Buttons.** Keep them active-low with the internal pull-up, or change
      `ui/buttons.rs` and the self-test's idle check together.
- [ ] **Display.** Confirm the module's I2C address and supply voltage; see
      [OLED Display](#oled-display).
- [ ] **Interrupt priorities.** Keep every application interrupt at 2, 3, 5,
      6 or 7, and set any new peripheral's priority explicitly as `main.rs`
      does for USBD and TWISPI0.
- [ ] **Clocks.** The internal RC low-frequency source works without a
      crystal. Switch to a crystal only with a matching accuracy setting and a
      hardware test. Check USB stability, given the open question above about
      the high-frequency crystal.
- [ ] **SoftDevice and flash map.** Use S140 v7.3.0, or update `memory_sd.x`
      to the other version's flash and RAM requirements. Check that nothing on
      the board, such as a factory bootloader or settings page, uses
      `0x000F0000–0x000F4000` or the rest of the application range. Keep
      `STORAGE_FLASH_PAGE_START`/`COUNT` and `memory_sd.x` in step.
- [ ] **Chip variant.** The build and `probe-rs` target `nRF52840_xxAA` with
      1 MiB flash and 256 KiB RAM. A different memory size needs new linker
      values.
- [ ] **USB identity.** Replace the development VID/PID before anything
      leaves the lab ([data model](data-model.md#usb-device-identity)).
- [ ] **Power.** Measure current with the new board and display, and decide
      whether the 100 mA declaration holds; see [Power](#power).
- [ ] **Re-verify.** Run `mask selftest`, record the SoftDevice RAM line and
      the stack high-water, run `mask size`, and complete the first-flash
      checklist with the board revision recorded.

A plug-in nRF52840 USB dongle keeps the same MCU and SoftDevice but has no
OLED, fewer buttons, and a preinstalled USB bootloader. On Nordic's nRF52840
Dongle that bootloader starts at `0xE0000`, inside today's application range
and below the pairing pages, so such a board needs its own pin map, a
display-less UI, and a different flash layout. It is tracked as the
[display-less dongle item](../TODO.md#more-peripherals-and-form-factors).

## Possible Future Ports

ESP32-S3, RP2040 with an external BLE module, and STM32 with an external BLE module
were previously listed as alternatives. They remain design ideas requiring
separate HAL, BLE, USB, pin, memory, and validation work. Only the nRF52840 build
is maintained here.

The parts of the code that are specific to the nRF52840 and would need
replacing in such a port:

| Area | nRF-specific dependency |
| --- | --- |
| BLE central, bonding, flash access | Nordic SoftDevice S140 through `nrf-softdevice` (vendored patch, [ADR 0007](adr/0007-vendored-softdevice-patch.md)) |
| USB device | `embassy-nrf` USBD driver with SoftDevice-fed VBUS detection |
| Unit serial | `FICR.DEVICEID` registers |
| Display bus | TWIM0 and the `StopSafeI2c` STOP handling written for it |
| Memory map | `memory_sd.x` and the SoftDevice RAM boundary assertion |
| Simulation | Renode's nRF52840 platform and the custom GPIO models |

The hardware-free modules (HID parsing and aggregation, coordinator reducers,
UI logic, storage framing) are shared through `src/lib.rs` and would carry
over ([ADR 0003](adr/0003-pure-core-and-task-shell.md)). A port is tracked as
the additional MCU item in [TODO.md](../TODO.md).

## Related Guides

- [First flash](first-flash.md)
- [Architecture and ADRs](architecture.md)
- [Data model](data-model.md)
- [Development](development.md)
- [Testing](testing.md)
- [Operations](operations.md)

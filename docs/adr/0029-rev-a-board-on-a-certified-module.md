# ADR 0029: Build The Rev A Board On A Certified nRF52840 Module With The Firmware's Pin Map

- Status: Accepted
- Date: 2026-11-09 (design review changes 2026-11-10)

## Context

Until now the only target was the nRF52840-DK with an OLED module and jumper
wires ([hardware](../hardware.md#parts-and-wiring)). A product that sits on a
monitor's hub needs its own board, and the open item "Production hardware
definition" in [TODO.md](../../TODO.md#board-bring-up-and-hardware-acceptance)
asks for one. The [custom board](../hardware.md#custom-board) requirements were
written first for a hardware partner; the owner then chose to hold the design
in this repository instead, so any fab can build it from production files.

Facts that constrain the design:

- **The firmware fixes the pins.** Buttons on P0.11, P0.12 and P0.24 to GND
  with internal pull-ups, I2C on P0.26 (SDA) and P0.27 (SCL) through TWIM0,
  P0.18 kept as pin reset, P0.09/P0.10 kept as NFC pins
  ([pin usage](../hardware.md#pin-and-peripheral-usage)). Changing them means
  changing `src/main.rs`, `src/selftest.rs` and the simulation together.
- **The firmware needs native USB with VBUS** on the nRF52840's VBUS pin, the
  32 MHz crystal, and no 32.768 kHz crystal
  ([clocks](../hardware.md#clocks-and-radio)).
- **The supply can be either mode.** Since 2026-11-09 the firmware sets UICR
  `REGOUT0` to 3.3 V, so 5 V into VDDH works, but a 3.3 V regulator into VDD
  works whatever UICR holds ([Supply Mode](../hardware.md#supply-mode)).
- **There is no bootloader.** Flashing and recovery use SWD only.
- **No RF expertise or radio test budget is available** for a first board.
- **KiCad 7.0.11 is what the project's tooling environment can install**; its
  command line has no ERC, and an external autorouter could not be run there.
  KiCad 9.0.9 later ran from its official container image, which has both
  ERC and DRC on the command line.

## Decision

- **Module.** Raytac MDBT50Q-1MV2: an nRF52840 module with a chip antenna and
  its own radio approvals, and a KiCad library symbol and footprint. Its
  footprint's antenna keep-out is extended to the board edge on both layers.
- **Pins.** Exactly the firmware's pin map, so rev A runs today's firmware
  unchanged. The module's inner-row pads (P0.11, P0.12, P0.18, P0.26) are
  reached by short tracks under the module, some through tented vias.
- **Supply.** Normal-voltage mode: an AP2112K-3.3 LDO feeds VDD, with VDDH
  tied to VDD; VBUS also goes to the module's VBUS pin, with 1 µF next to
  it. VBUS capacitance, 5.8 µF in all, stays under the USB 10 µF limit.
- **USB.** A 16-pin USB 2.0 Type-C receptacle (GCT USB4105-GF-A) with 5.1 kΩ
  CC pull-downs and a USBLC6-2SC6 ESD array next to it, with 100 nF on its
  VBUS pin. D+ and D− run side by side over the bottom ground plane.
- **User interface.** A 1×4 2.54 mm socket for a 0.96" SSD1306 I2C module,
  with two three-pad solder jumpers that set which of its first two pins is
  GND and which is VCC (GND first as delivered), 4.7 kΩ I2C pull-ups, and
  three 6 × 6 mm SMD tactile switches.
- **Debug.** A 2×5 1.27 mm ARM Cortex SWD header, plus test pads for P0.06,
  P0.08, 3V3, VBUS and GND.
- **Board.** Two layers, 74 × 32 mm, ground poured on both sides and
  stitched with vias, rules any common prototype fab meets (0.15 mm track
  and clearance, 0.3 mm drill).
- **Production files in the repository.** The KiCad project, a schematic PDF,
  Gerbers and drill files, a BOM and a placement file live in
  `hardware/bt2usb-board`, and the [hardware guide](../hardware.md#kicad-project-rev-a)
  documents them. The KiCad files, saved by KiCad 9, are the source; the fab
  files are exports that change with them.

## Alternatives Considered

- **Bare nRF52840 chip.** Cheaper per unit and smaller, but it needs Nordic's
  RF matching network and antenna, an RF layout review, and its own radio
  certification. Rejected for a first board.
- **Ebyte E73 or Minew MS88SF2 modules.** Also certified, but not in KiCad's
  library with USB broken out in a way checked here; they stay options in
  [Module Choice](../hardware.md#module-choice).
- **High-voltage mode without the LDO.** One part fewer, but VDD then depends
  on UICR, which a module maker's test firmware may already have set lower.
  Kept as a documented option.
- **Tag-Connect footprint instead of a header.** Smaller and free per board,
  but it needs a special cable; a 10-pin 1.27 mm header takes the cable that
  comes with common probes.
- **Leave design to a partner.** It hands the schematic and layout, and with
  them the product's details, to a third party, and the owner chose not to.
- **Change pins to the module's outer pads.** It would avoid the vias under
  the module but would change the firmware for a board that does not exist
  yet.
- **Solder the OLED module straight onto a header.** Lower, but the module's
  board would sit 2.5 mm above parts up to 1.45 mm tall with its own parts
  facing down, and a module with the other supply order would need a
  different board.

## Rationale

A certified module removes the parts of a radio product that need specialist
skills and equipment, at a cost of a few euros per unit. Keeping the
firmware's pins means the first board tests the firmware that already passed
host tests and simulation, not a port. An LDO in normal-voltage mode copies
the DK, the only supply arrangement the firmware was developed on. Files in
the repository let any fab in [Where To Build It](../hardware.md#where-to-build-it)
quote the same design.

## Consequences

- Rev A can be ordered from any prototype fab without design work.
- The tracks were placed by a grid router, not by hand: they pass DRC but are
  not as tidy as hand routing. The USB pair runs side by side but is long
  (about 56 mm) and not impedance-controlled, which full-speed USB tolerates.
- Vias sit under the module; they are tented, and the module has no exposed
  copper there other than its pads.
- The OLED module plugs into a socket over the supply parts, so its display
  sits about 13 to 14 mm above the board and about 9 mm above the switch
  tops; the enclosure needs button plungers
  ([Enclosure](../hardware.md#enclosure)).
- Raytac documents the MDBT50Q-1MV2 with a revision 2 nRF52840. The firmware
  runs on it, but the lock policy that [ADR 0021](0021-provisioning-debug-access-and-readout-protection.md)
  proposes for production needs revision 3, so production units need a
  revision 3 lot or another module.
- The production USB identity and readout protection are unchanged by this
  decision ([ADR 0018](0018-production-usb-identity.md),
  [ADR 0021](0021-provisioning-debug-access-and-readout-protection.md)).

## Implementation

`hardware/bt2usb-board` holds the project and its exports. Verification Status:

- **Software-verified:** the 2026-11-10 design review
  ([record](../validation-records.md#validation-record--2026-11-10-rev-a-design-review)):
  KiCad 9.0.9 ERC with no errors or warnings; KiCad 9.0.9 DRC at all
  severities with schematic parity, with no violations, unconnected pads,
  footprint or parity issues; the netlist compared with the pin map and the
  firmware pin for pin; module, regulator, ESD, switch, SWD and Type-C pins
  compared with their datasheets; the circuit compared with Raytac's
  reference circuit for supplies under 3.6 V. The earlier KiCad 7.0.11 DRC
  was clean too.
- **Not verified:** the module's pad sizes against Raytac's land-pattern
  drawing (only an image was available), a review by anyone other than the
  designer, and any built board.
- **Hardware-verified:** none; the first-flash record on a rev A board is the
  acceptance.

## Related

- [Hardware: custom board](../hardware.md#custom-board)
- [ADR 0002: nRF52840, SoftDevice and Embassy](0002-nrf52840-softdevice-embassy.md)
- [ADR 0012: bus-powered, no System-OFF](0012-bus-powered-no-system-off.md)
- [First flash](../first-flash.md)

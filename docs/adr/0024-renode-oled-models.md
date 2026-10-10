# ADR 0024: Model The TWIM And SSD1306 In Renode And Read The Panel's Text Back

- Status: Accepted
- Date: 2026-10-10

## Context

The Renode simulation ([ADR 0014](0014-renode-gpio-models.md)) is the only
automated check that runs firmware tasks on an ARM core. Until 2026-10-10 it
ran the button task, the UI controller, and the BLE and storage modules, but
not the display task. The "Broaden the Renode scenarios" item in
[TODO.md](../../TODO.md) accepted the work only when `mask sim-test` and the
CI simulation job also assert the OLED task's output.

The display task ([ADR 0009](0009-isolated-display-task.md)) drives an
SSD1306 through `embassy-nrf`'s `Twim` driver, wrapped in `StopSafeI2c`, with
the `ssd1306` 0.10 crate in buffered graphics mode. Three facts made it
unreachable in Renode 1.16.1:

- **No EasyDMA TWIM.** The stock `I2C.NRF52840_I2C` model at `0x40003000`
  implements only the legacy TWI: single bytes through `TXD` (`0x51C`) and
  `RXD` (`0x518`). `embassy-nrf` 0.7.0's `Twim` programs `TXD.PTR` and
  `TXD.MAXCNT`, triggers `TASKS_STARTTX`, and waits for `EVENTS_STOPPED`,
  `EVENTS_ERROR`, or `EVENTS_SUSPENDED` through the TWISPI0 interrupt
  (`setup_operations`, `async_wait`, and `InterruptHandler` in the crate's
  src/twim.rs), so on the stock model no transfer ever completes. The
  model's source on Renode's `master` branch, read on 2026-10-10, has no
  EasyDMA registers either.
- **No SSD1306.** The Renode 1.16.1 binary contains no SSD1306 model.
- **No way to check a picture.** Even with both models, a test needs to know
  what the panel shows. A pixel hash would break on every layout change and
  say nothing readable when it fails.

The screens were also drawn by code no host test reached: `draw_view` and
`draw_list` in `display.rs` mixed the layout (which text goes where) with the
`embedded-graphics` calls, inside a module the host library cannot compile.

## Decision

Run the firmware's display task in the simulation on two C# models written
for this project, read the panel's text back with the firmware's own font,
and assert that text in the Robot test.

- **Screen layout as data.** `ui::layout::lines`
  ([layout.rs](../../src/ui/layout.rs)) returns each screen's lines of text
  with their baselines. `display.rs` draws exactly those lines in `FONT_6X10`
  and nothing else. The module is in the pure core
  ([ADR 0003](0003-pure-core-and-task-shell.md)), with 12 host tests in
  [layout_tests.rs](../../src/ui/layout_tests.rs).
- **Shared TWIM setup and task.** `display::new_twim` configures TWIM0 (SDA
  P0.26, SCL P0.27, internal pull-ups, the 64-byte RAM copy buffer) and
  `display::task` runs `display::run` over `StopSafeI2c`. The bridge, the
  self-test, and the simulation all use `new_twim`; the bridge and the
  simulation spawn the same `task`. The bridge keeps raising the TWISPI0
  interrupt to priority 2 for the SoftDevice.
- **TWIM model.** `NRF52840_TWIM` in
  [nrf52840_twim.cs](../../renode/nrf52840_twim.cs) follows the nRF52840
  Product Specification's TWIM chapter:
  - `TASKS_STARTTX` and `TASKS_STARTRX` raise TXSTARTED or RXSTARTED, latch
    `PTR` and `MAXCNT` (they are double-buffered), move the bytes between Data
    RAM and the target with EasyDMA, set `AMOUNT`, and raise LASTTX or
    LASTRX; the `SHORTS` bits chain STARTRX, STARTTX, SUSPEND, or STOP after
    the last byte. A change of direction sends a repeated START and the
    address byte again.
  - STOP takes effect after the byte on the wire: the bytes already started
    are moved and counted, an address byte in flight is still NACKed, and
    STOPPED follows the STOP condition one bit time later, so the driver's
    error path really returns before STOPPED. STOP while suspended is
    ignored until RESUME, as the Product Specification requires. SUSPEND
    raises SUSPENDED and keeps the transaction open; a start task written
    while suspended runs on RESUME, as `embassy-nrf` writes them.
  - A transfer completes after the time its bytes take on the wire at
    `FREQUENCY` (9 bit times per byte plus the START bit and, for a new
    transaction, the address byte), so the driver really waits for the
    interrupt. At the firmware's 100 kHz a full frame takes about 0.1 s.
  - An address no target answers raises ERROR with `ERRORSRC.ANACK`, and the
    transaction waits for STOP. `twi0 SetDevicePresent <address> false` makes
    a registered target stop answering, as when it is unplugged.
  - A buffer outside Data RAM moves nothing and logs an error, because
    EasyDMA cannot read flash; firmware that relied on that would fail on the
    chip and fails here too.
  - The interrupt line is any enabled event among STOPPED, ERROR, SUSPENDED,
    RXSTARTED, TXSTARTED, LASTRX, and LASTTX.
- **SSD1306 model.** `SSD1306` in [ssd1306.cs](../../renode/ssd1306.cs)
  decodes the I2C control byte (Co and D/C#), the commands the `ssd1306`
  crate sends (and the rest of the datasheet's command set, with their
  argument counts), and keeps the 8 × 128-byte display RAM with the
  horizontal, vertical, and page addressing modes. It shows a picture only
  while the display and the charge pump are on, applies segment remap to the
  data written after it (as the datasheet's section 10.1.12 says), and COM
  scan direction, inverse, and entire-display-on at once. A multiplex ratio,
  display offset, start line, or COM pins configuration other than the
  128×64 defaults, or scrolling, logs a warning, and `Text` reports the
  setting instead of reading text the panel would not show that way. A
  power-on reset (`Reset`) turns the display off and fills the RAM with a
  fixed noise pattern; `NoisyBytes` counts the bytes the panel receives while
  it is lit and still shows some of that pattern.
- **Text reader.** `LoadFont` reads
  [oled-font-6x10.txt](../../renode/oled-font-6x10.txt), the 95 printable
  ASCII glyphs of `FONT_6X10`, which [oled_font.rs](../../tests/oled_font.rs)
  generates from `embedded-graphics` and keeps equal to it. `Text` scans the
  panel for rows where all 21 six-pixel cells match glyphs, returns those
  lines top to bottom, and reports any lit pixels no line explains, a dark
  panel, or a missing font in parentheses. `Dump` returns the panel as `#`
  and `.`.
- **Platform overlay.** [nrf52840-twim-oled.repl](../../renode/nrf52840-twim-oled.repl)
  declares `twi0` at `0x40003000` on `nvic@3`, the stock name, address, and
  IRQ, and `oled` at `0x3C` on it. The Robot test and
  [bt2usb-sim.resc](../../renode/bt2usb-sim.resc) include both C# files,
  unregister the stock `twi0` after loading the stock platform, load the
  overlay, and call `LoadFont`.
- **Robot checks.** `Oled Should Show` runs the emulation in 50 ms steps until
  the panel's text equals the expected lines. The test checks Home after boot
  and that no frame byte was written to a lit, noisy panel; every screen the
  scenario reaches; and a recovery: the panel stops answering, a press sends
  a frame that fails on the NACK, the panel is reset and answers again, shows
  `(display off)`, and then shows Home after the display task's 1 s retry,
  again without noise. A second test case checks the models themselves at
  register level against the Product Specification and the datasheet: STOP
  after the byte on the wire, STOP ignored while suspended, the suspended
  empty buffer, a buffer cut by STOP, double-buffered `MAXCNT`, the noise
  count, and the geometry report. The
  [OLED checks](../testing.md#oled-checks) list each one.

The first run of the noise check, which then counted frame bytes written to a
lit panel, failed with 1,024: the `ssd1306` crate's `init` ends by turning the
panel on, and the firmware then sent the frame, so a real panel showed its
power-on RAM for about 0.1 s at boot. Turning the panel off right after
`init` shortened that to the two bytes of the display-off command, which the
stricter count, any byte received while lit with noise, still reports. The
fix clears the RAM first: `display::initialize` selects horizontal
addressing and flushes a blank frame while power-up still holds the panel
off, then runs `init` and turns the panel off until `render` has sent the
frame. A render that initializes now sends two full frames, about 0.22 s at
100 kHz, inside the 500 ms deadline (FIXME "The OLED showed power-on noise
while its first frame was drawn" in [TODO.md](../../TODO.md)).

## Alternatives Considered

- **Compare frames with golden images.** A pixel-exact snapshot per screen,
  rendered on the host, would check the same pixels, but each layout change
  would mean regenerating images, and a failure would show two bitmaps
  instead of the text that differs.
- **Log what the firmware meant to draw.** Printing the layout's lines to
  UART would test `ui::layout` again, which host tests already do, and not
  the TWIM transfers, the `ssd1306` command stream, the addressing, or the
  panel's power state, which are what the simulation can add.
- **Simulation-only display code.** A sim build that skipped the I2C path or
  used the legacy TWI would pass in Renode while the firmware's `Twim` path
  stayed untested, against [ADR 0014](0014-renode-gpio-models.md)'s reason
  for modelling hardware instead.
- **Wait for upstream Renode models.** Renode has neither model today. Local
  models loaded at runtime work with the pinned Renode 1.16.1 and can be
  offered upstream later, as with the GPIO models.
- **Model the bus at the wire level** (SDA and SCL as GPIO). It would allow
  bus faults but would not match how `embassy-nrf` uses the peripheral, and
  it would cost far more C# for no extra check of the firmware.

## Rationale

The display task is the one firmware task whose correctness a user sees
first, and before this change the only checks were the host tests of its
retry policy and the board self-test. Modelling the two peripherals from
their specifications lets the simulation run the real driver stack, from
`Twim` through `StopSafeI2c` and the `ssd1306` crate to the panel's RAM, and
reading the text back with the firmware's own font gives assertions that read
like the screen and fail with the text that differs. Keeping the layout in a
pure module means each screen is checked twice: its lines on the host, and
those lines as pixels on the modelled panel.

## Consequences

Positive:

- The display task, the `ssd1306` driver, and the TWIM driver run in CI on
  every push, and the Robot test fails if a screen shows the wrong text, the
  panel stays dark, or the panel is ever lit while it shows power-on noise.
- The recovery path after an address NACK and the re-initialization on
  retry are checked. `StopSafeI2c`'s wait for STOPPED runs on that path (the
  model raises STOPPED after the driver has returned the error), but the
  test does not prove it: with the 1 s backoff before the next transfer, the
  test would pass without the wait.
- Each screen's layout is host-tested, and the layout tests check that every
  fixed label fits the panel's 21 columns and that lines never overlap.
- Developers can read the panel from the Renode monitor (`Text`, `Dump`).

Negative:

- About 1,300 more lines of C# to maintain against Renode's peripheral API;
  a Renode upgrade can break them, so change `RENODE_VERSION` only together
  with a passing simulation run.
- The models are written from the Product Specification and the SSD1306
  datasheet and checked only against the drivers that use them, not against
  silicon. The board's OLED stages and the first-flash checklist remain the
  hardware check.
- The TWIM model has no stuck bus, clock stretching, or data NACK, so the
  firmware's STOP request after the 500 ms deadline and `wait_stopped`'s
  re-request are still not exercised; only the model's own STOP handling is
  checked, at register level. It suspends after the current buffer, not the
  current byte, and does not tell the target about a repeated START.
- A change to the glyphs, such as an `embedded-graphics` upgrade, needs
  `UPDATE_OLED_FONT=1 cargo test --locked --test oled_font` to rewrite the
  glyph table; the test fails until then, and it also fails if `display.rs`
  draws in a font other than `FONT_6X10`. A font with another cell size
  needs the text reader changed too.
- The text reader assumes text drawn in 6-pixel cells from x = 0, as
  `ui::layout` draws it. A line made only of `-` reads as `_`, because those
  glyphs are the same bar at different heights.
- The simulation runs without the power manager, so the panel's auto-off and
  the suspend blanking are not exercised.

## Implementation

| Concern | Where |
| --- | --- |
| Screen layout | `lines` in [layout.rs](../../src/ui/layout.rs); tests in [layout_tests.rs](../../src/ui/layout_tests.rs) |
| Drawing, initialization, TWIM setup, task | `draw_view`, `initialize`, `new_twim`, and `task` in [display.rs](../../src/ui/display.rs) |
| Callers | [main.rs](../../src/main.rs), [selftest.rs](../../src/selftest.rs), [sim.rs](../../src/sim.rs) (which publishes the view after every loop iteration) |
| TWIM model | [nrf52840_twim.cs](../../renode/nrf52840_twim.cs) |
| SSD1306 model and text reader | [ssd1306.cs](../../renode/ssd1306.cs) |
| Glyph table and its check | [oled-font-6x10.txt](../../renode/oled-font-6x10.txt), [oled_font.rs](../../tests/oled_font.rs), `embedded-graphics` as a dev-dependency in [Cargo.toml](../../Cargo.toml) |
| Platform overlay | [nrf52840-twim-oled.repl](../../renode/nrf52840-twim-oled.repl) |
| Robot checks | `Oled Should Show`, `Unplug The OLED`, `Plug The OLED Back In`, and `Oled Showed No Power-On Noise` in the scenario test case, and the `TWIM And SSD1306 Models Follow Their Specifications` test case, in [bt2usb-sim.robot](../../renode/bt2usb-sim.robot) |
| Interactive script | [bt2usb-sim.resc](../../renode/bt2usb-sim.resc) |

### Verification Status

- **Implemented:** everything above.
- **Software-verified:** the 12 layout tests and 3 glyph-table tests pass
  with `cargo test`, and both Robot test cases passed locally with Renode
  1.16.1 on Linux
  ([2026-10-10 OLED record](../testing.md#validation-record--2026-10-10-oled-in-renode)).
  The noise check failed with 1,024 noisy frame bytes before the fix in
  `display::initialize`, and with 2 noisy bytes for a firmware that only
  turned the panel off after `init`.
- **Hardware-verified:** not yet. No board record exists for the first-flash
  OLED checks, including the absence of noise at power-up.

## Related

- [Testing: OLED checks](../testing.md#oled-checks)
- [Architecture: display update](../architecture.md#display-update)
- [Hardware: OLED display](../hardware.md#oled-display)
- [ADR 0003: Hardware-free decision modules](0003-pure-core-and-task-shell.md)
- [ADR 0009: Isolated display task](0009-isolated-display-task.md)
- [ADR 0014: Renode GPIO models](0014-renode-gpio-models.md)

*** Settings ***
Documentation     Headless Layer-3 test: boots the SoftDevice-free bt2usb-sim
...               firmware on a simulated nRF52840, presses the real GPIO
...               buttons, and asserts over UART0 that the firmware's UI
...               controller, coordinator reducers, scan merging, link-loss
...               reservation, saved-device management, and pairing-store codec
...               all run on the target, and that the display task draws each
...               screen's text on the simulated OLED. A second test checks
...               the TWIM model itself against the Product Specification.
...               Run with:  renode-test renode/bt2usb-sim.robot
Suite Setup       Setup And Compile Models
Suite Teardown    Teardown
Test Teardown     Test Teardown
Resource          ${RENODEKEYWORDS}

*** Variables ***
# Override with: renode-test --variable ELF:/abs/path renode/bt2usb-sim.robot
${ELF}            ${CURDIR}/../target/thumbv7em-none-eabihf/debug/bt2usb-sim
# Base platform; override (e.g. --variable PLATFORM:@/abs/nrf52840.repl) to use
# a local copy.
${PLATFORM}       @platforms/cpus/nrf52840.repl

# Button pins (active-low, pulled up): drive `false` to press, `true` to release.
${PIN_UP}         11
${PIN_DOWN}       12
${PIN_SELECT}     24

# The SSD1306 model, attached to TWIM0 at 0x3C by nrf52840-twim-oled.repl.
${OLED}           sysbus.twi0.oled

# TWIM0 registers (nRF52840 Product Specification, TWIM chapter).
${TASKS_STARTTX}      0x40003008
${TASKS_STOP}         0x40003014
${TASKS_SUSPEND}      0x4000301C
${TASKS_RESUME}       0x40003020
${EVENTS_STOPPED}     0x40003104
${EVENTS_ERROR}       0x40003124
${EVENTS_SUSPENDED}   0x40003148
${EVENTS_LASTTX}      0x40003160
${SHORTS}             0x40003200
${ERRORSRC}           0x400034C4
${TXD_MAXCNT}         0x40003548
${TXD_AMOUNT}         0x4000354C
${LASTTX_SUSPEND}     0x100
${LASTTX_STOP}        0x200

*** Keywords ***
Setup And Compile Models
    # Renode compiles each C# model once per process; the types stay
    # registered across the emulation resets between test cases.
    Setup
    Execute Command           include @${CURDIR}/nrf52840_sense_gpio.cs
    Execute Command           include @${CURDIR}/nrf52840_twim.cs
    Execute Command           include @${CURDIR}/ssd1306.cs

Create Sim Machine
    # nRF52840 with GPIO/GPIOTE models that implement pin SENSE/LATCH and the
    # GPIOTE PORT event, so injected edges reach embassy-nrf's async edge waits.
    Execute Command           mach create "bt2usb-sim"
    Execute Command           machine LoadPlatformDescription ${PLATFORM}
    Execute Command           sysbus Unregister sysbus.gpiote
    Execute Command           sysbus Unregister sysbus.gpio0
    Execute Command           sysbus Unregister sysbus.gpio1
    Execute Command           machine LoadPlatformDescription @${CURDIR}/nrf52840-sense-gpio.repl
    # An EasyDMA TWIM in place of the legacy TWI, with the SSD1306 at 0x3C, and
    # the firmware's font for reading the panel's text back.
    Execute Command           sysbus Unregister sysbus.twi0
    Execute Command           machine LoadPlatformDescription @${CURDIR}/nrf52840-twim-oled.repl
    Execute Command           ${OLED} LoadFont @${CURDIR}/oled-font-6x10.txt
    Execute Command           sysbus LoadELF @${ELF}

Press Button
    [Documentation]    Press the button on ${pin}, wait for the UI loop to log
    ...                ${expected}, then release it and let the release settle.
    ...                Emulation is paused while the pin is driven, so edges land
    ...                at deterministic points; the 100 ms after the release
    ...                outlasts the 50 ms debounce, so the same button can be
    ...                pressed again next. Each press restarts the sim's 2 s
    ...                scenario timer, so a sequence of presses is never
    ...                interrupted by a scenario step.
    [Arguments]        ${pin}    ${expected}
    Execute Command           gpio0 OnGPIO ${pin} false
    Wait For Line On Uart     ${expected}    pauseEmulation=true
    Execute Command           gpio0 OnGPIO ${pin} true
    Execute Command           emulation RunFor "0.1"

Oled Should Show
    [Documentation]    Run the emulation in 50 ms steps, up to ${steps} of them,
    ...                until the text read back from the simulated OLED is
    ...                exactly ${lines}, one argument per line of text. A full
    ...                frame at the firmware's 100 kHz I2C clock takes ~0.12 s.
    [Arguments]        @{lines}    ${steps}=10
    ${expected}=              Catenate    SEPARATOR=\n    @{lines}
    Wait Until Keyword Succeeds    ${steps}x    0 s    Oled Text After 50 ms Should Be    ${expected}

Oled Text After 50 ms Should Be
    [Arguments]        ${expected}
    Execute Command           emulation RunFor "0.05"
    ${text}=                  Execute Command    ${OLED} Text
    Should Be Equal           ${text.rstrip()}    ${expected}

Unplug The OLED
    [Documentation]    The panel stops answering its address, as when unplugged.
    Execute Command           twi0 SetDevicePresent 0x3C false

Plug The OLED Back In
    [Documentation]    A replugged panel powers up: display off, defaults
    ...                restored, RAM holding power-on noise.
    Execute Command           ${OLED} Reset
    Execute Command           twi0 SetDevicePresent 0x3C true

Oled Showed No Power-On Noise
    [Documentation]    The firmware never lit the panel while its RAM still
    ...                held any of the content it powered up with.
    Oled Should Count Noisy Bytes    0

Oled Should Count Noisy Bytes
    [Arguments]        ${expected}
    ${noisy}=                 Execute Command    ${OLED} NoisyBytes
    Should Be Equal As Integers    ${noisy.strip()}    ${expected}

Create Bus Machine
    [Documentation]    The TWIM model and the OLED, with its font, on a halted
    ...                nRF52840, set up as embassy-nrf leaves TWIM0 for the
    ...                display: enabled, 100 kHz, address 0x3C, and a 2-byte
    ...                command in Data RAM.
    Execute Command           mach create "twim-check"
    Execute Command           machine LoadPlatformDescription ${PLATFORM}
    Execute Command           sysbus Unregister sysbus.twi0
    Execute Command           machine LoadPlatformDescription @${CURDIR}/nrf52840-twim-oled.repl
    Execute Command           ${OLED} LoadFont @${CURDIR}/oled-font-6x10.txt
    Execute Command           cpu IsHalted true
    Write Register            0x40003500    6
    Write Register            0x40003524    0x01980000
    Write Register            0x40003588    0x3C
    Write Register            0x40003544    0x20000000
    Write Register            0x20000000    0x0000AE00

Write Register
    [Arguments]        ${address}    ${value}
    Execute Command           sysbus WriteDoubleWord ${address} ${value}

Register Should Be
    [Arguments]        ${address}    ${expected}
    ${value}=                 Execute Command    sysbus ReadDoubleWord ${address}
    Should Be Equal As Integers    ${value.strip()}    ${expected}

Clear Twim Events
    FOR    ${event}    IN    ${EVENTS_STOPPED}    ${EVENTS_ERROR}    ${EVENTS_SUSPENDED}    ${EVENTS_LASTTX}
        Write Register        ${event}    0
    END
    Write Register            ${ERRORSRC}    6

Run For 1 ms
    Execute Command           emulation RunFor "0.001"

Send To Oled
    [Documentation]    Write the first ${count} bytes of ${word} (little-endian)
    ...                to the OLED in one I2C transaction.
    [Arguments]        ${word}    ${count}
    Write Register            0x20000000    ${word}
    Write Register            ${SHORTS}    ${LASTTX_STOP}
    Write Register            ${TXD_MAXCNT}    ${count}
    Write Register            ${TASKS_STARTTX}    1
    Run For 1 ms
    Register Should Be        ${EVENTS_STOPPED}    1
    Clear Twim Events

Oled Text Should Be
    [Arguments]        ${expected}
    ${text}=                  Execute Command    ${OLED} Text
    Should Be Equal           ${text.rstrip()}    ${expected}

*** Test Cases ***
Sim Runs The UI Controller, Display, Coordinator, Management, And Store
    Create Sim Machine
    Create Terminal Tester    sysbus.uart0    timeout=20
    Start Emulation

    # Assertions are in chronological emission order: `Wait For Line On Uart`
    # consumes the stream sequentially, so each line must come after the prior.
    #
    # Boot + executor reached the UI loop.
    Wait For Line On Uart     bt2usb-sim starting
    # The build script's identity (src/diagnostics.rs): a full commit, or
    # `unknown` for a build outside git.
    Wait For Line On Uart     version \\d+\\.\\d+\\.\\d+, commit ([0-9a-f]{40}(-dirty)?|unknown), (debug|release) build    treatAsRegex=true
    # The MPU stack guard (src/stack.rs) is on; the overflow test checks it.
    Wait For Line On Uart     stack guard: 4096 bytes at 0x[0-9a-f]{8}\\.\\.0x[0-9a-f]{8}    treatAsRegex=true
    Wait For Line On Uart     buttons ready
    Wait For Line On Uart     display task started
    Wait For Line On Uart     entering sim UI loop (screen=Home)    pauseEmulation=true

    # The display task initializes the panel and draws Home, keeping it dark
    # until the frame has replaced the power-on RAM content.
    Oled Should Show          bt2usb / Idle    SELECT: scan    UP: saved devices
    Oled Showed No Power-On Noise

    # Scenario step 0 (t~2s, RTC time driver): the keyboard connects through
    # plan_connect and on_slot_connected, is saved, and the saved item reads
    # back. Pause there so the presses below land before the next step.
    Wait For Line On Uart     scenario: connect device 0 (Keyboard)
    Wait For Line On Uart     action: ConnectSlot slot=0 addr=0xa1
    Wait For Line On Uart     action: PersistDevice addr=0xa1
    Wait For Line On Uart     holds 1 device(s); reload matches
    Wait For Line On Uart     scenario: active_count=1 occupied_count=1
    Wait For Line On Uart     event: Connected 'Keyboard' -> screen Connected (selected 0)    pauseEmulation=true
    Oled Should Show          Connected    Keyboard    SEL:add DOWN:disc    UP:saved devices

    # A user scan from Connected, driven by real GPIO presses (P0.24 SELECT,
    # P0.12 DOWN, P0.11 UP) through ui::buttons. The scan merges three
    # advertisements and lists only the two HID devices.
    Press Button              ${PIN_SELECT}    button Select -> screen Scanning (selected 0)
    Wait For Line On Uart     cmd: StartScan
    Wait For Line On Uart     scan: heard 3 advertisers, listed 2 HID devices
    Wait For Line On Uart     event: ScanStarted -> screen Scanning
    Wait For Line On Uart     event: DeviceFound 'Keyboard' addr=0xa1 rssi=-42
    Wait For Line On Uart     event: DeviceFound 'Mouse' addr=0xb2 rssi=-55
    Wait For Line On Uart     event: ScanComplete -> screen DeviceList (selected 0)    pauseEmulation=true
    Oled Should Show          Select device    > Keyboard    ${SPACE * 2}Mouse
    Press Button              ${PIN_DOWN}      button Down -> screen DeviceList (selected 1)
    Oled Should Show          Select device    ${SPACE * 2}Keyboard    > Mouse
    Press Button              ${PIN_UP}        button Up -> screen DeviceList (selected 0)
    Press Button              ${PIN_DOWN}      button Down -> screen DeviceList (selected 1)

    # Connect the mouse: a slot is reserved, connected, and saved.
    Press Button              ${PIN_SELECT}    button Select -> screen Connecting (selected 1)
    Wait For Line On Uart     cmd: Connect(1)
    Wait For Line On Uart     action: ConnectSlot slot=1 addr=0xb2
    Wait For Line On Uart     holds 2 device(s); reload matches
    Wait For Line On Uart     event: Connected '2 devices' -> screen Connected (selected 0)    pauseEmulation=true
    Oled Should Show          Connected    2 devices    SEL:add DOWN:disc    UP:saved devices

    # Step 1: the mouse is already connected, so plan_connect only reports the
    # links again. Step 2: the keyboard's link drops; on_slot_link_lost keeps
    # its slot reserved for the reconnect, and the UI shows only the mouse.
    Wait For Line On Uart     scenario: connect device 1 (Mouse)
    Wait For Line On Uart     scenario: active_count=2 occupied_count=2
    Wait For Line On Uart     scenario: slot 0 link lost
    Wait For Line On Uart     scenario: slot 0 kept reserved for 0xa1
    Wait For Line On Uart     scenario: active_count=1 occupied_count=2
    Wait For Line On Uart     event: Connected 'Mouse' -> screen Connected (selected 0)    pauseEmulation=true
    Oled Should Show          Connected    Mouse    SEL:add DOWN:disc    UP:saved devices

    # Saved devices, newest first; cancelling a Forget changes nothing.
    Press Button              ${PIN_UP}        button Up -> screen Managing (selected 0)
    Wait For Line On Uart     cmd: ListPaired id=1
    Wait For Line On Uart     event: PairedDevices id=1 ['Mouse', 'Keyboard'] -> screen SavedDevices (selected 0)    pauseEmulation=true
    Oled Should Show          Saved devices    > Mouse    ${SPACE * 2}Keyboard    ${SPACE * 2}Factory reset    UP at first: back
    Press Button              ${PIN_DOWN}      button Down -> screen SavedDevices (selected 1)
    Press Button              ${PIN_SELECT}    button Select -> screen ConfirmForget(1) (selected 0)
    Oled Should Show          Forget device?    Keyboard    > Cancel    ${SPACE * 2}Forget
    Press Button              ${PIN_SELECT}    button Select -> screen SavedDevices (selected 0)

    # Forget the keyboard while its slot is reserved: the barrier releases that
    # slot only, the shorter list is saved and reads back, the mouse stays up.
    Press Button              ${PIN_DOWN}      button Down -> screen SavedDevices (selected 1)
    Press Button              ${PIN_SELECT}    button Select -> screen ConfirmForget(1) (selected 0)
    Press Button              ${PIN_DOWN}      button Down -> screen ConfirmForget(1) (selected 1)
    Oled Should Show          Forget device?    Keyboard    ${SPACE * 2}Cancel    > Forget
    Press Button              ${PIN_SELECT}    button Select -> screen Managing (selected 1)
    Wait For Line On Uart     cmd: Forget id=2 addr=0xa1
    Wait For Line On Uart     quiesce: slot 0 released
    Wait For Line On Uart     quiesce: complete for token 1
    Wait For Line On Uart     holds 1 device(s); reload matches
    Wait For Line On Uart     slots: active_count=1 occupied_count=1
    Wait For Line On Uart     event: Connected 'Mouse' -> screen Managing
    Wait For Line On Uart     event: ManagementResult id=2 Ok(()) -> screen Notice    pauseEmulation=true
    Oled Should Show          Complete    Device forgotten    SELECT: back
    Press Button              ${PIN_SELECT}    button Select -> screen Connected (selected 0)

    # Factory reset from the last entry of the list: every slot is released,
    # the empty list is saved, and the UI returns home with no link.
    Press Button              ${PIN_UP}        button Up -> screen Managing (selected 0)
    Wait For Line On Uart     cmd: ListPaired id=3
    Wait For Line On Uart     event: PairedDevices id=3 ['Mouse'] -> screen SavedDevices (selected 0)
    Press Button              ${PIN_DOWN}      button Down -> screen SavedDevices (selected 1)
    Press Button              ${PIN_SELECT}    button Select -> screen ConfirmReset (selected 0)
    Press Button              ${PIN_DOWN}      button Down -> screen ConfirmReset (selected 1)
    Oled Should Show          Reset all pairings?    Disconnect all    ${SPACE * 2}Cancel    > Reset
    Press Button              ${PIN_SELECT}    button Select -> screen Managing (selected 1)
    Wait For Line On Uart     cmd: FactoryReset id=4
    Wait For Line On Uart     quiesce: slot 1 released
    Wait For Line On Uart     quiesce: complete for token 2
    Wait For Line On Uart     holds 0 device(s); reload matches
    Wait For Line On Uart     slots: active_count=0 occupied_count=0
    Wait For Line On Uart     event: Disconnected -> screen Managing
    Wait For Line On Uart     event: ManagementResult id=4 Ok(()) -> screen Notice    pauseEmulation=true
    Oled Should Show          Complete    Pairings reset    SELECT: back

    # The OLED drops off the bus as the UI goes home: the frame fails on an
    # address NACK, and the UI loop carries on. Plugged back in, the panel is
    # off with power-on RAM; the display task's retry (1 s after the failure)
    # initializes it again and draws the latest view, still keeping it dark
    # until that frame is in.
    Unplug The OLED
    Press Button              ${PIN_SELECT}    button Select -> screen Home (selected 0)
    Plug The OLED Back In
    ${text}=                  Execute Command    ${OLED} Text
    Should Be Equal           ${text.rstrip()}    (display off)
    Oled Should Show          bt2usb / Idle    SELECT: scan    UP: saved devices    steps=40
    Oled Showed No Power-On Noise

    # Step 3 has no link left to close; the next cycle's keyboard is saved
    # again, so the reset store accepts writes.
    Wait For Line On Uart     scenario: disconnect all
    Wait For Line On Uart     scenario: active_count=0 occupied_count=0
    Wait For Line On Uart     scenario: connect device 0 (Keyboard)
    Wait For Line On Uart     holds 1 device(s); reload matches
    Wait For Line On Uart     event: Connected 'Keyboard' -> screen Connected (selected 0)    pauseEmulation=true
    Oled Should Show          Connected    Keyboard    SEL:add DOWN:disc    UP:saved devices

A Stack Overflow Faults In The Guard And Is Reported
    # The firmware has no code path that overflows, so the test moves the
    # stack pointer 256 bytes into the guard while the core sleeps between
    # tasks, as a function whose frame no longer fits would leave it. The
    # next interrupt's frame lands in the no-access MPU region and faults,
    # and the fault handler reports the overflow on UART0 and stops the core
    # (src/stack.rs, docs/adr/0026-mpu-stack-guard.md). Renode takes the
    # fault as MemManage, not HardFault; both paths reach the same report.
    Create Sim Machine
    Create Terminal Tester    sysbus.uart0    timeout=20
    Start Emulation
    ${guard}=                 Wait For Line On Uart    stack guard: 4096 bytes at (0x[0-9a-f]{8})\\.\\.(0x[0-9a-f]{8})    treatAsRegex=true
    ${base}=                  Set Variable    ${guard.Groups[0]}
    ${top}=                   Set Variable    ${guard.Groups[1]}
    Wait For Line On Uart     entering sim UI loop (screen=Home)    pauseEmulation=true
    # The display task has drawn Home by then and the next scenario step is
    # about 1 s away, so the core is asleep in the executor.
    Execute Command           emulation RunFor "1"
    ${sp}=                    Evaluate    hex(int('${top}', 16) - 256)
    Execute Command           sysbus.cpu SP ${sp}
    Wait For Line On Uart     stack overflow: stack pointer 0x[0-9a-f]{8}, guard ${base}\\.\\.${top}, PC (0x[0-9a-f]{8}|not stacked)    treatAsRegex=true
    # The core spins in the handler: the 2 s scenario steps never come.
    Should Not Be On Uart     scenario:    timeout=5

TWIM And SSD1306 Models Follow Their Specifications
    # Register-level checks of renode/nrf52840_twim.cs and renode/ssd1306.cs
    # for behavior the firmware's display traffic does not reach, against the
    # nRF52840 Product Specification's TWIM chapter and the SSD1306 datasheet.
    Create Bus Machine

    # A STOP takes effect after the byte on the wire: an empty write to an
    # absent target (STARTTX then STOP at once, as embassy-nrf sends an empty
    # buffer) still NACKs its address, and STOPPED follows the STOP condition.
    Execute Command           twi0 SetDevicePresent 0x3C false
    Write Register            ${TXD_MAXCNT}    0
    Write Register            ${TASKS_STARTTX}    1
    Write Register            ${TASKS_STOP}    1
    Register Should Be        ${EVENTS_STOPPED}    0
    Run For 1 ms
    Register Should Be        ${EVENTS_ERROR}    1
    Register Should Be        ${ERRORSRC}    2
    Register Should Be        ${EVENTS_STOPPED}    1
    Register Should Be        ${TXD_AMOUNT}    0
    Execute Command           twi0 SetDevicePresent 0x3C true
    Clear Twim Events

    # The TWIM cannot be stopped while suspended: STOP is ignored until RESUME.
    Write Register            ${SHORTS}    ${LASTTX_SUSPEND}
    Write Register            ${TXD_MAXCNT}    2
    Write Register            ${TASKS_STARTTX}    1
    Run For 1 ms
    Register Should Be        ${EVENTS_SUSPENDED}    1
    Register Should Be        ${TXD_AMOUNT}    2
    Write Register            ${TASKS_STOP}    1
    Run For 1 ms
    Register Should Be        ${EVENTS_STOPPED}    0
    Write Register            ${TASKS_RESUME}    1
    Write Register            ${TASKS_STOP}    1
    Run For 1 ms
    Register Should Be        ${EVENTS_STOPPED}    1
    Clear Twim Events

    # An empty buffer suspended by firmware (embassy-nrf's empty write before
    # another write) raises SUSPENDED after the address byte.
    Write Register            ${TXD_MAXCNT}    0
    Write Register            ${TASKS_STARTTX}    1
    Write Register            ${TASKS_SUSPEND}    1
    Run For 1 ms
    Register Should Be        ${EVENTS_SUSPENDED}    1
    Write Register            ${TASKS_RESUME}    1
    Write Register            ${TASKS_STOP}    1
    Run For 1 ms
    Register Should Be        ${EVENTS_STOPPED}    1
    Clear Twim Events

    # STOP during a buffer: at 100 kHz, 500 us is the START bit, the address,
    # and four data bytes, with the fifth on the wire. That byte finishes,
    # AMOUNT counts five, and LASTTX never comes.
    Write Register            ${SHORTS}    0
    Write Register            ${TXD_MAXCNT}    17
    Write Register            ${TASKS_STARTTX}    1
    Execute Command           emulation RunFor "0.0005"
    Write Register            ${TASKS_STOP}    1
    Run For 1 ms
    Register Should Be        ${TXD_AMOUNT}    5
    Register Should Be        ${EVENTS_LASTTX}    0
    Register Should Be        ${EVENTS_STOPPED}    1
    Clear Twim Events

    # TXD.MAXCNT is double-buffered: a new value written after STARTTX is for
    # the next transfer.
    Write Register            ${SHORTS}    ${LASTTX_STOP}
    Write Register            ${TXD_MAXCNT}    2
    Write Register            ${TASKS_STARTTX}    1
    Write Register            ${TXD_MAXCNT}    9
    Run For 1 ms
    Register Should Be        ${TXD_AMOUNT}    2
    Register Should Be        ${EVENTS_LASTTX}    1
    Register Should Be        ${EVENTS_STOPPED}    1
    Clear Twim Events

    # A panel lit before its RAM is cleared shows power-on noise, and every
    # byte it receives meanwhile counts: here the control byte and a NOP.
    Execute Command           ${OLED} Reset
    Oled Text Should Be       (display off)
    Send To Oled              0xAF148D00    4
    Oled Should Count Noisy Bytes    0
    Send To Oled              0x0000E300    2
    Oled Should Count Noisy Bytes    2
    Oled Text Should Be       (unreadable pixels in rows 0-63)

    # Geometry other than the 128x64 defaults is reported, not read: COM pins
    # 0x02 (sequential) would interleave the rows on this module.
    Send To Oled              0x0002DA00    3
    Oled Text Should Be       (picture not modelled: COM pins 0x02)
    Send To Oled              0x0012DA00    3
    Oled Text Should Be       (unreadable pixels in rows 0-63)

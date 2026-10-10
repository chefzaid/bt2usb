*** Settings ***
Documentation     Headless Layer-3 test: boots the SoftDevice-free bt2usb-sim
...               firmware on a simulated nRF52840, presses the real GPIO
...               buttons, and asserts over UART0 that the firmware's UI
...               controller, coordinator reducers, scan merging, link-loss
...               reservation, saved-device management, and pairing-store codec
...               all run on the target.
...               Run with:  renode-test renode/bt2usb-sim.robot
Suite Setup       Setup
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

*** Keywords ***
Create Sim Machine
    # nRF52840 with GPIO/GPIOTE models that implement pin SENSE/LATCH and the
    # GPIOTE PORT event, so injected edges reach embassy-nrf's async edge waits.
    Execute Command           include @${CURDIR}/nrf52840_sense_gpio.cs
    Execute Command           mach create "bt2usb-sim"
    Execute Command           machine LoadPlatformDescription ${PLATFORM}
    Execute Command           sysbus Unregister sysbus.gpiote
    Execute Command           sysbus Unregister sysbus.gpio0
    Execute Command           sysbus Unregister sysbus.gpio1
    Execute Command           machine LoadPlatformDescription @${CURDIR}/nrf52840-sense-gpio.repl
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

*** Test Cases ***
Sim Runs The UI Controller, Coordinator, Management, And Store
    Create Sim Machine
    Create Terminal Tester    sysbus.uart0    timeout=20
    Start Emulation

    # Assertions are in chronological emission order: `Wait For Line On Uart`
    # consumes the stream sequentially, so each line must come after the prior.
    #
    # Boot + executor reached the UI loop.
    Wait For Line On Uart     bt2usb-sim starting
    Wait For Line On Uart     buttons ready
    Wait For Line On Uart     entering sim UI loop (screen=Home)

    # Scenario step 0 (t~2s, RTC time driver): the keyboard connects through
    # plan_connect and on_slot_connected, is saved, and the saved item reads
    # back. Pause there so the presses below land before the next step.
    Wait For Line On Uart     scenario: connect device 0 (Keyboard)
    Wait For Line On Uart     action: ConnectSlot slot=0 addr=0xa1
    Wait For Line On Uart     action: PersistDevice addr=0xa1
    Wait For Line On Uart     holds 1 device(s); reload matches
    Wait For Line On Uart     scenario: active_count=1 occupied_count=1
    Wait For Line On Uart     event: Connected 'Keyboard' -> screen Connected (selected 0)    pauseEmulation=true

    # A user scan from Connected, driven by real GPIO presses (P0.24 SELECT,
    # P0.12 DOWN, P0.11 UP) through ui::buttons. The scan merges three
    # advertisements and lists only the two HID devices.
    Press Button              ${PIN_SELECT}    button Select -> screen Scanning (selected 0)
    Wait For Line On Uart     cmd: StartScan
    Wait For Line On Uart     scan: heard 3 advertisers, listed 2 HID devices
    Wait For Line On Uart     event: ScanStarted -> screen Scanning
    Wait For Line On Uart     event: DeviceFound 'Keyboard' addr=0xa1 rssi=-42
    Wait For Line On Uart     event: DeviceFound 'Mouse' addr=0xb2 rssi=-55
    Wait For Line On Uart     event: ScanComplete -> screen DeviceList (selected 0)
    Press Button              ${PIN_DOWN}      button Down -> screen DeviceList (selected 1)
    Press Button              ${PIN_UP}        button Up -> screen DeviceList (selected 0)
    Press Button              ${PIN_DOWN}      button Down -> screen DeviceList (selected 1)

    # Connect the mouse: a slot is reserved, connected, and saved.
    Press Button              ${PIN_SELECT}    button Select -> screen Connecting (selected 1)
    Wait For Line On Uart     cmd: Connect(1)
    Wait For Line On Uart     action: ConnectSlot slot=1 addr=0xb2
    Wait For Line On Uart     holds 2 device(s); reload matches
    Wait For Line On Uart     event: Connected '2 devices' -> screen Connected (selected 0)

    # Step 1: the mouse is already connected, so plan_connect only reports the
    # links again. Step 2: the keyboard's link drops; on_slot_link_lost keeps
    # its slot reserved for the reconnect, and the UI shows only the mouse.
    Wait For Line On Uart     scenario: connect device 1 (Mouse)
    Wait For Line On Uart     scenario: active_count=2 occupied_count=2
    Wait For Line On Uart     scenario: slot 0 link lost
    Wait For Line On Uart     scenario: slot 0 kept reserved for 0xa1
    Wait For Line On Uart     scenario: active_count=1 occupied_count=2
    Wait For Line On Uart     event: Connected 'Mouse' -> screen Connected (selected 0)    pauseEmulation=true

    # Saved devices, newest first; cancelling a Forget changes nothing.
    Press Button              ${PIN_UP}        button Up -> screen Managing (selected 0)
    Wait For Line On Uart     cmd: ListPaired id=1
    Wait For Line On Uart     event: PairedDevices id=1 ['Mouse', 'Keyboard'] -> screen SavedDevices (selected 0)
    Press Button              ${PIN_DOWN}      button Down -> screen SavedDevices (selected 1)
    Press Button              ${PIN_SELECT}    button Select -> screen ConfirmForget(1) (selected 0)
    Press Button              ${PIN_SELECT}    button Select -> screen SavedDevices (selected 0)

    # Forget the keyboard while its slot is reserved: the barrier releases that
    # slot only, the shorter list is saved and reads back, the mouse stays up.
    Press Button              ${PIN_DOWN}      button Down -> screen SavedDevices (selected 1)
    Press Button              ${PIN_SELECT}    button Select -> screen ConfirmForget(1) (selected 0)
    Press Button              ${PIN_DOWN}      button Down -> screen ConfirmForget(1) (selected 1)
    Press Button              ${PIN_SELECT}    button Select -> screen Managing (selected 1)
    Wait For Line On Uart     cmd: Forget id=2 addr=0xa1
    Wait For Line On Uart     quiesce: slot 0 released
    Wait For Line On Uart     quiesce: complete for token 1
    Wait For Line On Uart     holds 1 device(s); reload matches
    Wait For Line On Uart     slots: active_count=1 occupied_count=1
    Wait For Line On Uart     event: Connected 'Mouse' -> screen Managing
    Wait For Line On Uart     event: ManagementResult id=2 Ok(()) -> screen Notice
    Press Button              ${PIN_SELECT}    button Select -> screen Connected (selected 0)

    # Factory reset from the last entry of the list: every slot is released,
    # the empty list is saved, and the UI returns home with no link.
    Press Button              ${PIN_UP}        button Up -> screen Managing (selected 0)
    Wait For Line On Uart     cmd: ListPaired id=3
    Wait For Line On Uart     event: PairedDevices id=3 ['Mouse'] -> screen SavedDevices (selected 0)
    Press Button              ${PIN_DOWN}      button Down -> screen SavedDevices (selected 1)
    Press Button              ${PIN_SELECT}    button Select -> screen ConfirmReset (selected 0)
    Press Button              ${PIN_DOWN}      button Down -> screen ConfirmReset (selected 1)
    Press Button              ${PIN_SELECT}    button Select -> screen Managing (selected 1)
    Wait For Line On Uart     cmd: FactoryReset id=4
    Wait For Line On Uart     quiesce: slot 1 released
    Wait For Line On Uart     quiesce: complete for token 2
    Wait For Line On Uart     holds 0 device(s); reload matches
    Wait For Line On Uart     slots: active_count=0 occupied_count=0
    Wait For Line On Uart     event: Disconnected -> screen Managing
    Wait For Line On Uart     event: ManagementResult id=4 Ok(()) -> screen Notice
    Press Button              ${PIN_SELECT}    button Select -> screen Home (selected 0)

    # Step 3 has no link left to close; the next cycle's keyboard is saved
    # again, so the reset store accepts writes.
    Wait For Line On Uart     scenario: disconnect all
    Wait For Line On Uart     scenario: active_count=0 occupied_count=0
    Wait For Line On Uart     scenario: connect device 0 (Keyboard)
    Wait For Line On Uart     holds 1 device(s); reload matches
    Wait For Line On Uart     event: Connected 'Keyboard' -> screen Connected (selected 0)

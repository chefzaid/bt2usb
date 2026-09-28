*** Settings ***
Documentation     Headless Layer-3 test: boots the SoftDevice-free bt2usb-sim
...               firmware on a simulated nRF52840, presses the real GPIO
...               buttons, and asserts that both pure cores (ble::coordinator and
...               ui::ui_logic) run on the target, observed over UART0.
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
    ...                ${expected}, then release it. Emulation is paused while
    ...                the pin is driven, so edges land at deterministic points.
    [Arguments]        ${pin}    ${expected}
    Execute Command           gpio0 OnGPIO ${pin} false
    Wait For Line On Uart     ${expected}    pauseEmulation=true
    Execute Command           gpio0 OnGPIO ${pin} true

*** Test Cases ***
Sim Boots And Runs Coordinator And UI Logic
    Create Sim Machine
    Create Terminal Tester    sysbus.uart0    timeout=20
    Start Emulation

    # Assertions are in chronological emission order: `Wait For Line On Uart`
    # consumes the stream sequentially, so each line must come after the prior.
    #
    # Boot + executor reached the UI loop.
    Wait For Line On Uart     bt2usb-sim starting
    Wait For Line On Uart     buttons ready
    Wait For Line On Uart     entering sim UI loop

    # ble::coordinator: first device connects (t~2s). Pause there so the
    # button presses below all land before the next scenario step.
    Wait For Line On Uart     action: UI Connected 'Keyboard'    pauseEmulation=true

    # ui::ui_logic driven by real GPIO button presses (P0.24 SELECT, P0.12
    # DOWN, P0.11 UP), through ui::buttons and embassy-nrf's edge detection.
    # Home --SELECT--> Scanning; the sim's scan completes at once -> DeviceList.
    Press Button              ${PIN_SELECT}    button Select -> screen Scanning (selected 0)
    Wait For Line On Uart     cmd: StartScan
    Wait For Line On Uart     scan: 2 devices -> screen DeviceList
    # DeviceList navigation.
    Press Button              ${PIN_DOWN}      button Down -> screen DeviceList (selected 1)
    Wait For Line On Uart     redraw: DeviceList
    Press Button              ${PIN_UP}        button Up -> screen DeviceList (selected 0)
    Wait For Line On Uart     redraw: DeviceList
    # Connect the highlighted device.
    Press Button              ${PIN_SELECT}    button Select -> screen Connecting (selected 0)
    Wait For Line On Uart     cmd: Connect(0)

    # ble::coordinator: second device connects -> two active links.
    Wait For Line On Uart     action: UI Connected '2 devices'
    Wait For Line On Uart     scenario: active_count=2

    # Teardown path: all links dropped.
    Wait For Line On Uart     action: UI Disconnected
    Wait For Line On Uart     scenario: active_count=0

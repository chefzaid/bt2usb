*** Settings ***
Documentation     Headless Layer-3 test: boots the SoftDevice-free bt2usb-sim
...               firmware on a simulated nRF52840 and asserts that both pure
...               cores (ble::coordinator and ui::ui_logic) run on the target,
...               observed over UART0. Run with:  renode-test renode/bt2usb-sim.robot
Suite Setup       Setup
Suite Teardown    Teardown
Test Teardown     Test Teardown
Resource          ${RENODEKEYWORDS}

*** Variables ***
# Override with: renode-test --variable ELF:/abs/path renode/bt2usb-sim.robot
${ELF}            ${CURDIR}/../target/thumbv7em-none-eabihf/debug/bt2usb-sim

*** Test Cases ***
Sim Boots And Runs Coordinator And UI Logic
    Execute Command           mach create "bt2usb-sim"
    Execute Command           machine LoadPlatformDescription @platforms/cpus/nrf52840.repl
    Execute Command           sysbus LoadELF @${ELF}
    Create Terminal Tester    sysbus.uart0    timeout=20
    Start Emulation

    # Assertions are in chronological emission order: `Wait For Line On Uart`
    # consumes the stream sequentially, so each line must come after the prior.
    #
    # Boot + executor reached the UI loop.
    Wait For Line On Uart     bt2usb-sim starting
    Wait For Line On Uart     entering sim UI loop

    # ble::coordinator: first device connects (t~2s).
    Wait For Line On Uart     action: UI Connected 'Keyboard'

    # ui::ui_logic: a SELECT press is reduced to a scan command (t~3s).
    Wait For Line On Uart     button Select -> screen Scanning
    Wait For Line On Uart     cmd: StartScan

    # ble::coordinator: second device connects -> two active links (t~4s).
    Wait For Line On Uart     action: UI Connected '2 devices'
    Wait For Line On Uart     scenario: active_count=2

    # Teardown path: all links dropped (t~8s).
    Wait For Line On Uart     action: UI Disconnected
    Wait For Line On Uart     scenario: active_count=0

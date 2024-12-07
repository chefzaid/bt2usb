# bt2usb
Bluetooth KVM - Converts Bluetooth signals to USB

## Context

- Connect a Bluetooth keyboard/mouse to any monitor with a USB port  
- Allows switching between multiple PCs using the same Bluetooth keyboard/mouse without pairing/unpairing
- Switch PCs by:
  - Plugging/unplugging the monitor’s USB connection to the desired PC, or  
  - Using a keyboard shortcut if the monitor supports it (e.g. Dell Display Manager)

## Hardware

- Micro Controller Unit (e.g., Arduino, STM32)  
- Bluetooth Module  
- Small Screen - OLED/TFT 128x128 (e.g. SSD1306)  
- Buttons: up, down, OK, pairing mode  
- Optional Buzzer for pairing sounds  
- USB-A male port to connect to the monitor  
- 3D-printed plastic case for protection

## Software

- Flash an RTOS with Bluetooth support on the MCU  
- Native Rust application for BT/USB conversion  
- Basic terminal UI to select devices

## Process

1. Press the "pairing mode" button to start scanning  
2. Press the pairing button on the device (keyboard/mouse)  
3. Detected devices are listed on the device's screen  
4. Select the device and confirm with "OK"

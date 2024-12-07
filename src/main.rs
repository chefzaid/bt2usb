use anyhow::Result;
use btleplug::api::{Central, Characteristic, Manager as _, Peripheral as _};
use btleplug::platform::{Manager, Peripheral};
use futures::stream::StreamExt;
use hidapi::HidApi;
use std::time::Duration;
use tokio::time::sleep;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize HID API for USB output (Example: USB keyboard)
    let hid_api = HidApi::new()?;
    let vendor_id = 0x046d;  
    let product_id = 0xc31c;

    let hid_device = match hid_api.open(vendor_id, product_id) {
        Ok(dev) => dev,
        Err(e) => {
            eprintln!("Failed to open USB HID device: {}", e);
            return Ok(());
        }
    };

    // TODO show a Terminal UI that lists all devices
    // TODO let user choose and confirm, once done, pair with that BLE device

    // Initialize Bluetooth manager and get the first adapter
    let manager = Manager::new().await?;
    let adapters = manager.adapters().await?;
    let adapter = match adapters.get(0) {
        Some(ad) => ad.to_owned(),
        None => {
            eprintln!("No Bluetooth adapter found");
            return Ok(());
        }
    };

    // Scan for devices and wait a few seconds
    adapter.start_scan(Default::default()).await?;
    sleep(Duration::from_secs(5)).await;

    // Find a peripheral that looks like a keyboard/mouse by name
    let mut kb_device: Option<Peripheral> = None;
    for p in adapter.peripherals().await? {
        if let Some(props) = p.properties().await? {
            if let Some(name) = props.local_name {
                let name_lower = name.to_lowercase();
                if name_lower.contains("keyboard") || name_lower.contains("mouse") {
                    kb_device = Some(p);
                    break;
                }
            }
        }
    }

    let peripheral = match kb_device {
        Some(p) => p,
        None => {
            eprintln!("No Bluetooth keyboard/mouse device found.");
            return Ok(());
        }
    };

    if !peripheral.is_connected().await? {
        peripheral.connect().await?;
    }
    peripheral.discover_services().await?;

    // Find the HID input characteristic (commonly 0x2A4D)
    let hid_input_char = find_hid_input_characteristic(&peripheral).await?;
    let hid_input_char = match hid_input_char {
        Some(c) => c,
        None => {
            eprintln!("No HID input characteristic found on the device.");
            return Ok(());
        }
    };

    let mut notification_stream = peripheral.notifications().await?;
    peripheral.subscribe(&hid_input_char).await?;
    println!("Subscribed to HID input notifications. Listening...");

    while let Some(data) = notification_stream.next().await {
        if data.uuid == hid_input_char.uuid {
            let report = data.value;

            let translated_report = translate_standard_bt_report_to_usb(&report);

            if let Err(e) = hid_device.write(&translated_report) {
                eprintln!("Failed to write to USB device: {}", e);
            }
        }
    }

    Ok(())
}

fn translate_standard_bt_report_to_usb(input: &[u8]) -> Vec<u8> {
    // In many standard cases, BLE and USB HID keyboard/mouse reports are identical or very similar.
    // If needed, adjust or parse the input and build a USB-specific report.
    // For now, we pass it through unchanged.
    input.to_vec()
}

async fn find_hid_input_characteristic(
    peripheral: &impl btleplug::api::Peripheral
) -> Result<Option<Characteristic>> {
    let services = peripheral.services();
    for service in services {
        for characteristic in &service.characteristics {
            // Common HID input characteristic UUID: 0x2A4D
            if characteristic.uuid.to_string().to_lowercase().contains("2a4d") {
                return Ok(Some(characteristic.clone()));
            }
        }
    }
    Ok(None)
}

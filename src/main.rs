use anyhow::Result;
use btleplug::api::{Central, Characteristic, Manager as _, Peripheral as _};
use btleplug::platform::{Manager, Peripheral};
use futures::stream::StreamExt;
// use hidapi::HidApi;
use std::time::Duration;
use tokio::time::sleep;

#[tokio::main]
async fn main() -> Result<()> {

    // This part is for connecting already paired devices
    /* 
    let hid_api = HidApi::new()?;
    // Microsoft Keyboard Surface:
    let vendor_id = 0x045e;  
    let product_id = 0x0917;

    let hid_device = match hid_api.open(vendor_id, product_id) {
        Ok(dev) => dev,
        Err(e) => {
            eprintln!("Failed to open USB HID device: {}", e);
            return Ok(());
        }
    };
    */

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
    sleep(Duration::from_secs(10)).await;

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

    let peripheral: Peripheral = match kb_device {
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


    list_all_bt_devices(&peripheral).await?;

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
            /*
            let translated_report = translate_standard_bt_report_to_usb(&report);
            
            if let Err(e) = hid_device.write(&translated_report) {
                eprintln!("Failed to write to USB device: {}", e);
            }
            */
        }
    }

    Ok(())
}

/*
fn translate_standard_bt_report_to_usb(input: &[u8]) -> Vec<u8> {
    // In many standard cases, BLE and USB HID keyboard/mouse reports are identical or very similar.
    // If needed, adjust or parse the input and build a USB-specific report.
    // For now, we pass it through unchanged.
    input.to_vec()
}
 */

async fn list_all_bt_devices(peripheral: &Peripheral) -> Result<()> {
    // List all devices local names and UUID Characteristics
    if let Some(props) = peripheral.properties().await? {
        let local_name = props.local_name.unwrap_or_else(|| "Unnamed".into());
        println!("Device Local Name: {}", local_name);
    } else {
        println!("Device properties not available");
    }
    for service in peripheral.services() {
        println!("Service UUID: {}", service.uuid);
        for characteristic in &service.characteristics {
            println!("  Characteristic UUID: {}", characteristic.uuid);
        }
    }
    Ok(())
}

async fn find_hid_input_characteristic(
    peripheral: &impl btleplug::api::Peripheral
) -> Result<Option<Characteristic>> {
    let services = peripheral.services();
    for service in services {
        for characteristic in &service.characteristics {
            // Common HID input characteristic UUID: 0x2A4D
            if characteristic.uuid.to_string().to_lowercase().contains("2a4d") { // TODO Specify the correct value for characteristic (2a4d)
                return Ok(Some(characteristic.clone()));
            }
        }
    }
    Ok(None)
}

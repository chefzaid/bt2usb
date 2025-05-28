use anyhow::Result;
use btleplug::api::{Central, Characteristic, Manager as _, Peripheral as _};
use btleplug::platform::{Manager, Peripheral};
use futures::stream::StreamExt;

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

    // Show Terminal UI to select device
    let peripheral = select_bluetooth_device(&adapter).await?;

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
            let _report = data.value;
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

async fn select_bluetooth_device(adapter: &btleplug::platform::Adapter) -> Result<Peripheral> {
    use std::io::{self, Write};
    use std::time::Duration;
    use tokio::time::sleep;

    println!("Scanning for Bluetooth devices...");

    // Start scanning for devices
    adapter.start_scan(Default::default()).await?;
    sleep(Duration::from_secs(5)).await;

    // Get all discovered peripherals
    let peripherals = adapter.peripherals().await?;

    if peripherals.is_empty() {
        return Err(anyhow::anyhow!("No Bluetooth devices found"));
    }

    // Collect device information
    let mut devices = Vec::new();
    for peripheral in peripherals {
        if let Ok(Some(props)) = peripheral.properties().await {
            let name = props.local_name.unwrap_or_else(|| "Unknown Device".to_string());
            let address = props.address.to_string();
            devices.push((peripheral, name, address));
        }
    }

    if devices.is_empty() {
        return Err(anyhow::anyhow!("No devices with readable properties found"));
    }

    // Display device selection menu
    loop {
        println!("\n=== Available Bluetooth Devices ===");
        for (i, (_, name, address)) in devices.iter().enumerate() {
            println!("{}. {} ({})", i + 1, name, address);
        }
        println!("\nEnter device number (1-{}) or 'q' to quit: ", devices.len());

        io::stdout().flush()?;

        // Read user input
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let input = input.trim();

        if input.eq_ignore_ascii_case("q") {
            return Err(anyhow::anyhow!("User cancelled device selection"));
        }

        if let Ok(choice) = input.parse::<usize>() {
            if choice > 0 && choice <= devices.len() {
                let (peripheral, name, address) = &devices[choice - 1];

                // Confirm selection
                println!("\nSelected: {} ({})", name, address);
                println!("Connect to this device? (y/n): ");
                io::stdout().flush()?;

                let mut confirm = String::new();
                io::stdin().read_line(&mut confirm)?;

                if confirm.trim().eq_ignore_ascii_case("y") {
                    println!("Connecting to {}...", name);
                    return Ok(peripheral.clone());
                }
            } else {
                println!("Invalid choice. Please enter a number between 1 and {}", devices.len());
            }
        } else {
            println!("Invalid input. Please enter a number or 'q' to quit.");
        }
    }
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
    use uuid::Uuid;

    // HID Report characteristic UUID (0x2A4D)
    let hid_report_uuid = Uuid::parse_str("00002a4d-0000-1000-8000-00805f9b34fb")?;

    let services = peripheral.services();
    for service in services {
        for characteristic in &service.characteristics {
            if characteristic.uuid == hid_report_uuid {
                return Ok(Some(characteristic.clone()));
            }
        }
    }
    Ok(None)
}

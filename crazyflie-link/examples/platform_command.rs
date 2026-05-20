use anyhow::{anyhow, Result};
use crazyflie_link::LinkContext;

/// Platform command types handled by the nRF radio chip
const PLATFORM_SYSTEM: u8 = 0xFE;

/// System sub-commands
const PLATFORM_SYSTEM_CMD_GETVBAT: u8 = 0x04;

#[tokio::main]
async fn main() -> Result<()> {
    let context = LinkContext::new();

    // Scan for a Crazyflie
    let found = context.scan([0xe7; 5]).await?;
    let uri = found
        .iter()
        .find(|u| u.starts_with("radio://"))
        .ok_or_else(|| anyhow!("No Crazyflie with radio:// URI found"))?;
    println!("Found Crazyflie at {}", uri);

    // Query battery voltage from the nRF chip.
    // This is a platform command handled entirely by the nRF radio chip
    // and never forwarded to the STM32 application processor.
    // The nRF responds directly in the radio ACK with the pre-sampled
    // battery voltage.
    let ack = context
        .platform_command(
            uri,
            vec![0xFF, PLATFORM_SYSTEM, PLATFORM_SYSTEM_CMD_GETVBAT],
        )
        .await?;

    if ack.received && ack.data.len() >= 7 {
        // Response: [0xFF, 0xFE, 0x04, vbat_f32_le...]
        let vbat = f32::from_le_bytes(ack.data[3..7].try_into()?);
        println!("Battery voltage: {:.2} V", vbat);
    } else {
        println!(
            "No response to battery query (ack received: {})",
            ack.received
        );
    }

    // Display radio link info from the ack
    if let Some(rssi) = ack.rssi_dbm {
        println!("RSSI: {} dBm", rssi);
    }
    println!("Retries: {}", ack.retry);

    Ok(())
}

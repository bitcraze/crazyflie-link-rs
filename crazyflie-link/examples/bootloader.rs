use anyhow::{anyhow, Result};
use async_std::future::timeout;
use byteorder::{ByteOrder, LittleEndian};
use crazyflie_link::{Connection, LinkContext, Packet};
use std::time::Duration;
use structopt::StructOpt;

const TARGET_STM32: u8 = 0xFF;
const TARGET_NRF51: u8 = 0xFE;

#[derive(StructOpt, Debug)]
#[structopt(name = "bootloader")]
struct Opt {
    #[structopt(short, long)]
    warm: bool,

    #[structopt(name = "URI")]
    uri: Option<String>,
}

#[derive(Debug)]
struct BootloaderInfo {
    id: u8,
    protocol_version: u8,
    page_size: u16,
    buffer_pages: u16,
    flash_pages: u16,
    start_page: u16,
    cpuid: u16,
}

async fn scan_for_bootloader() -> Result<String> {
    let context = crate::LinkContext::new(async_executors::AsyncStd);
    let res = context
        .scan_selected(vec![
            "radio://0/110/2M/E7E7E7E7E7",
            "radio://0/0/2M/E7E7E7E7E7",
        ])
        .await?;

    if res.is_empty() {
        Ok(String::from(""))
    } else {
        Ok(String::from(&res[0]))
    }
}

async fn get_info(link: &Connection, target: u8) -> Result<BootloaderInfo> {
    for _ in 0..5 {
        let packet: Packet = vec![0xFF, target, 0x10].into();

        link.send_packet(packet).await?;
        let packet = timeout(Duration::from_millis(100), link.recv_packet())
            .await?
            .unwrap();
        let data = packet.get_data();

        if packet.get_header() == 0xFF && data.len() >= 2 && data[0..2] == [target, 0x10] {
            return Ok(BootloaderInfo {
                id: data[0],
                protocol_version: data[1],
                page_size: LittleEndian::read_u16(&data[2..4]),
                buffer_pages: LittleEndian::read_u16(&data[4..6]),
                flash_pages: LittleEndian::read_u16(&data[6..8]),
                start_page: LittleEndian::read_u16(&data[8..10]),
                cpuid: LittleEndian::read_u16(&data[10..12]),
            });
        }
    }

    Err(anyhow!("Failed to get info"))
}

async fn reset_to_bootloader(link: &Connection) -> Result<String> {
    let packet: Packet = vec![0xFF, TARGET_NRF51, 0xFF].into();
    link.send_packet(packet).await?;

    let mut new_address = Vec::new();
    loop {
        let packet = timeout(Duration::from_millis(100), link.recv_packet())
            .await?
            .unwrap();
        let data = packet.get_data();
        if data.len() > 2 && data[0..2] == [TARGET_NRF51, 0xFF] {
            new_address.push(0xb1);
            for byte in data[2..6].iter().rev() {
                // handle little-endian order
                new_address.push(*byte);
            }
            break;
        }
    }

    for _ in 0..10 {
        let packet: Packet = vec![0xFF, TARGET_NRF51, 0xF0, 0x00].into();
        link.send_packet(packet).await?;
    }
    async_std::task::sleep(Duration::from_secs(1)).await;

    Ok(format!(
        "radio://0/0/2M/{}?safelink=0&ackfilter=0",
        hex::encode(new_address).to_uppercase()
    ))
}

async fn start_bootloader(context: &LinkContext, warm: bool, uri: &str) -> Result<Connection> {
    let uri = if warm {
        let link = context.open_link(&format!("{}?safelink=0", uri)).await?;
        let uri = reset_to_bootloader(&link).await;
        link.close().await;
        async_std::task::sleep(Duration::from_secs(1)).await;
        uri
    } else {
        scan_for_bootloader().await
    }?;

    let link = context.open_link(&uri).await?;
    Ok(link)
}

#[async_std::main]
async fn main() -> Result<()> {
    let opt = Opt::from_args();
    let context = LinkContext::new(async_executors::AsyncStd);
    let mut uri = String::new();

    if opt.warm {
        uri = match opt.uri {
            Some(uri) => uri,
            None => {
                eprintln!("no uri supplied for warm reset to bootloader");
                std::process::exit(1);
            }
        };
    }

    let link = start_bootloader(&context, opt.warm, &uri).await?;

    if let Ok(info) = get_info(&link, TARGET_STM32).await {
        println!("\n== stm32 ==\n{:#?}", info);
    }

    if let Ok(info) = get_info(&link, TARGET_NRF51).await {
        println!("\n== nrf51 ==\n{:#?}", info);
    }

    Ok(())
}

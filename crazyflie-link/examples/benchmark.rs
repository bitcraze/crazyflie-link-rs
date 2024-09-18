// Simple benchmark test, test ping time and duplex bandwidth
use tokio::time::timeout;
use crazyflie_link::{LinkContext, Packet};
use futures::Future;
use std::{
    ops::Div,
    time::{Duration, Instant},
};
use structopt::StructOpt;

#[derive(StructOpt, Debug)]
#[structopt(name = "bandwidth")]
struct Opt {
    #[structopt(short, default_value = "1000")]
    n_packets: u32,

    #[structopt(short, default_value = "28")]
    size_packet: usize,

    #[structopt(name = "URI")]
    link_uri: String,
}

async fn bench<F: Future<Output = anyhow::Result<()>>>(function: F) -> anyhow::Result<Duration> {
    let start = Instant::now();
    function.await?;
    Ok(Instant::now() - start)
}

async fn purge_crazyflie_queues(link: &crazyflie_link::Connection) {
    // Purge crazyflie queues
    loop {
        if timeout(std::time::Duration::from_millis(10), link.recv_packet())
            .await
            .is_err()
        {
            break;
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let opt = Opt::from_args();

    let link_context = LinkContext::new();
    let link = link_context.open_link(&opt.link_uri).await?;

    println!("Test ping:");
    purge_crazyflie_queues(&link).await;
    let mut ping_times = Vec::new();
    for i in 0..opt.n_packets {
        let mut packet = Packet::new(0xF, 0, vec![i as u8]);
        let ping_time = bench(async {
            link.send_packet(packet).await?;
            loop {
                packet = timeout(std::time::Duration::from_secs(10), link.recv_packet())
                    .await?
                    .unwrap();
                if packet.get_header() == 0xf0 {
                    break;
                }
            }

            let data = packet.get_data();
            if data[0] != (i as u8) {
                println!(
                    "Communication error! Expected {}, received {}.",
                    i as u8, data[0]
                );
                panic!();
            }

            Ok(())
        })
        .await?;

        ping_times.push(ping_time);
    }

    let ping_mean = ping_times
        .iter()
        .fold(Duration::from_nanos(0), |acc, v| acc + *v)
        .div(ping_times.len() as u32);
    let min_ping = ping_times.iter().min().unwrap();
    let max_ping = ping_times.iter().max().unwrap();

    println!(
        "Mean ping time: {:?}, min: {:?}, max: {:?}",
        ping_mean, min_ping, max_ping
    );

    println!("Test bandwidth:");
    purge_crazyflie_queues(&link).await;

    let runtime = bench(async {
        for i in (0..opt.n_packets).into_iter() {
            let packet = Packet::new(0xF, 0, vec![i as u8]);
            link.send_packet(packet).await?;
        }
        for i in (0..opt.n_packets).into_iter() {
            let mut packet;

            loop {
                packet = timeout(std::time::Duration::from_secs(10), link.recv_packet())
                    .await?
                    .unwrap();
                if packet.get_header() == 0xf0 {
                    break;
                }
            }

            let data = packet.get_data();
            if data[0] != (i as u8) {
                println!(
                    "Communication error! Expected {}, received {}.",
                    i as u8, data[0]
                );
                panic!();
            }
        }

        Ok(())
    })
    .await?;

    let packet_rate = (opt.n_packets as f64) / runtime.as_secs_f64();
    let bandwidth = packet_rate * (opt.size_packet as f64);

    println!(
        "Runtime: {:?}, packet rate: {:.4} pk/s, bandwidth: {:.4} kB/s",
        runtime,
        packet_rate,
        bandwidth / 1024.
    );

    Ok(())
}

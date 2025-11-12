// Simple benchmark test, test ping time and duplex bandwidth
use tokio::time::timeout;
use crazyflie_link::{LinkContext, Packet};
use futures::{Future, future::join_all};
use std::{
    ops::Div,
    time::{Duration, Instant},
};
use structopt::StructOpt;

#[derive(StructOpt, Debug)]
#[structopt(name = "bandwidth", about = "Benchmark Crazyflie link bandwidth and ping time. Multiple links URI can be specified.")]
struct Opt {
    #[structopt(short, default_value = "1000", help = "Number of benchmark packets")]
    b_packets: u32,

    #[structopt(short, default_value = "1000", help = "Number of ping packets")]
    p_packets: u32,

    #[structopt(short, default_value = "28", help = "Size of each packet's payloadin bytes")]
    size_packet: usize,

    #[structopt(name = "URI")]
    link_uri: Vec<String>,
}

async fn bench<F: Future<Output = anyhow::Result<()>>>(function: F) -> anyhow::Result<Duration> {
    let start = Instant::now();
    function.await?;
    Ok(Instant::now() - start)
}

async fn purge_crazyflie_queues(link: &crazyflie_link::Connection) {
    // Purge crazyflie queues
    loop {
        if timeout(std::time::Duration::from_millis(100), link.recv_packet())
            .await
            .is_err()
        {
            break;
        }
    }
}

async fn benchmark(id: String, link: crazyflie_link::Connection, p_packets: u32, b_packets: u32, size_packet: usize) {

    println!("Test ping for {}:", id);
    purge_crazyflie_queues(&link).await;
    let mut ping_times = Vec::new();
    for i in 0..p_packets {
        let mut packet = Packet::new(0xF, 0, vec![i as u8]);
        let ping_time = bench(async {
            link.send_packet(packet).await.unwrap();

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
        .await.unwrap();

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

    println!("Test bandwidth for {}:", id);
    purge_crazyflie_queues(&link).await;

    let runtime = bench(async {
        for i in (0..b_packets).into_iter() {
            let packet = Packet::new(0xF, 0, vec![i as u8]);
            link.send_packet(packet).await.unwrap();
        }
        for i in (0..b_packets).into_iter() {
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
    .await.unwrap();

    let packet_rate = (b_packets as f64) / runtime.as_secs_f64();
    let bandwidth = packet_rate * (size_packet as f64);

    println!(
        "Runtime: {:?}, packet rate: {:.4} pk/s, bandwidth: {:.4} kB/s",
        runtime,
        packet_rate,
        bandwidth / 1024.
    );

}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let opt = Opt::from_args();

    let link_context = LinkContext::new();

    dbg!(&opt.link_uri);

    let mut benchmarks = Vec::new();
    for uri in opt.link_uri {
        let link = link_context.open_link(&uri).await?;
        benchmarks.push(benchmark(format!("{}", uri), link, opt.p_packets, opt.b_packets, opt.size_packet));
    }

    join_all(benchmarks).await;

    Ok(())
}

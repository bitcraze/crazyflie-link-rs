// Simple bandwidth test
use crazyflie_link::LinkContext;
use std::time::{Instant, Duration};
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

fn bench<F: FnOnce() -> anyhow::Result<()>>(function: F) -> anyhow::Result<Duration> {
    let start = Instant::now();
    function()?;
    Ok(Instant::now() - start)
}

fn main() -> anyhow::Result<()> {

    let opt = Opt::from_args();

    let link_context = LinkContext::new();
    let link = link_context.open_link(&opt.link_uri)?;


    // Purge crazyflie queues
    loop {
        if let Err(_) = link.recv_packet_timeout(Duration::from_millis(100)) {
            break;
        }
    }


    let runtime = bench( || {
        for i in (0..opt.n_packets).into_iter() {
            let mut packet = vec![0;opt.size_packet];
            packet[0] = 0xf0; // Echo port
            packet[1] = i as u8;
            link.send_packet(packet)?;
        }
        for i in (0..opt.n_packets).into_iter() {
            let mut packet;
            
            loop {
                packet = link.recv_packet_timeout(std::time::Duration::from_secs(10))?;
                // println!("{:?}", packet);
                if packet[0] == 0xf0 {
                    break;
                }
            }
    
            if packet[1] != (i as u8) {
                println!("Communication error! Expected {}, received {}.", i as u8, packet[1]);
                panic!();
            }
        }

        Ok(())
    })?.as_secs_f64();
    


    let packet_rate = (opt.n_packets as f64)/runtime;
    let bandwidth = packet_rate * (opt.size_packet as f64);


    println!("Runtime: {}, packet rate: {} pk/s, bandwidth: {} kB/s", runtime, packet_rate, bandwidth / 1024.);

    Ok(())
}
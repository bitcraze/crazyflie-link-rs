use crazyflie_link::LinkContext;

fn main() -> anyhow::Result<()> {
    let link_context = LinkContext::new();

    let link = link_context.open_link("radio://0/60/2M/E7E7E7E7E7")?;

    loop {
        let packet = link.recv_packet_timeout(std::time::Duration::from_secs(10))?;
        if packet.starts_with(&[0]) {
            let line = String::from_utf8_lossy(&packet[1..]);
            print!("{}", line);
        }
    }
}

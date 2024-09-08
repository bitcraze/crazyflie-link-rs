use tokio::time::{Duration, timeout};
use crazyflie_link::LinkContext;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let link_context = LinkContext::new();

    let link = link_context.open_link("radio://0/60/2M/E7E7E7E7E7").await?;

    loop {
        let packet = timeout(Duration::from_secs(10), link.recv_packet())
            .await?
            .unwrap();
        let data = packet.get_data();
        if packet.get_header() == 0 {
            let line = String::from_utf8_lossy(&data[0..]);
            print!("{}", line);
        }
    }
}

use std::sync::Arc;
use tokio::time::{sleep, Duration};

use crazyflie_link::LinkContext;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let link_context = LinkContext::new();

    println!("Connecting the first time");
    let link = Arc::new(link_context.open_link("radio://0/60/2M/E7E7E7E7E7").await?);

    let link_task = link.clone();
    tokio::spawn(async move {
        let reason = link_task.wait_close().await;
        println!(
            " -= after wait_close() =- The link seem to have been closed for reason \"{}\"!",
            reason
        );
    });

    sleep(Duration::from_secs(2)).await;

    println!("Closing the connection");
    link.close().await;
    println!("Waiting 3 seconds");
    sleep(Duration::from_secs(3)).await;

    println!("Connecting the second time");
    let link = link_context.open_link("radio://0/60/2M/E7E7E7E7E7").await?;

    sleep(Duration::from_secs(2)).await;

    println!("Dropping link object");
    drop(link);
    println!("Waiting 3 seconds");
    sleep(Duration::from_secs(3)).await;

    println!("Connecting and dropping directly");
    let link = link_context.open_link("radio://0/60/2M/E7E7E7E7E7").await?;
    drop(link);

    println!("Waiting 3 seconds");
    sleep(Duration::from_secs(3)).await;

    Ok(())
}

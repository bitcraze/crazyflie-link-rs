use anyhow::Result;
use crazyflie_link::LinkContext;

#[tokio::main]
async fn main() -> Result<()> {
    let context = crate::LinkContext::new();

    let found = context.scan([0xe7; 5]).await?;

    println!("Found {} Crazyflies.", found.len());
    for uri in found {
        println!(" - {}", uri)
    }

    Ok(())
}

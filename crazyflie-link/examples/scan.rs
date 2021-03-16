use anyhow::Result;
use crazyflie_link::LinkContext;

fn main() -> Result<()> {
    let context = crate::LinkContext::new();

    let found = context.scan([0xe7; 5])?;

    println!("Found {} Crazyflies.", found.len());
    for uri in found {
        println!(" - {}", uri)
    }

    Ok(())
}

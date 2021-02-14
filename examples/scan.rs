use crazyflie_link::LinkContext;
use anyhow::Result;

fn main() -> Result<()> {
    let context = crate::LinkContext::new()?;

    let found = context.scan()?;

    println!("Found {} Crazyflies.", found.len());
    for uri in found {
        println!(" - {}", uri)
    }
    
    Ok(())
}

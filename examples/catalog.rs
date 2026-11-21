//! Read-only: the account, then GPU types with their stock and prices,
//! cheapest secure-cloud first.
//!
//! ```sh
//! RUNPOD_API_KEY=... cargo run --example catalog
//! ```

use runpod::{CatalogInclude, Client, ListGpuTypesParams};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let runpod = Client::new(std::env::var("RUNPOD_API_KEY")?)?;

    let account = runpod.get_account().await?;
    let balance = runpod.get_balance().await?;
    println!("account {} — balance {:?}", account.id, balance);

    let mut gpus = runpod
        .list_gpu_types(&ListGpuTypesParams {
            include: Some(vec![CatalogInclude::Availability]),
            ..Default::default()
        })
        .await?
        .gpus;
    gpus.retain(|g| g.secure);
    gpus.sort_by(|a, b| a.price.secure.total_cmp(&b.price.secure));

    println!("{:<32} {:>6} {:>10}  availability", "GPU", "GB", "$/h");
    for g in gpus {
        let availability = g.availability.map(|a| a.to_string()).unwrap_or_default();
        println!(
            "{:<32} {:>6} {:>10.3}  {availability}",
            g.id, g.memory, g.price.secure
        );
    }
    if let Some(rl) = runpod.rate_limit() {
        println!("\nquota: {:?}", rl.windows);
    }
    Ok(())
}

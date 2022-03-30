#[deny(unused)]
use std::thread;
use tracing_subscriber;

use tracing::info;

#[tokio::main]
pub async fn main() -> server::Result<()> {
    tracing_subscriber::fmt::try_init();

    let host = "localhost:25568";
    info!("starting server on {}", host);
    //Blocks this thread here as server.start runs in another thread
    thread::park();
    Ok(())
}

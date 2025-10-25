use blockchain_sim::MyBlockchain;
use common::{Config, logger_init};
use tonic::transport::Server;

#[tokio::main]
pub async fn main() -> Result<(), Box<dyn std::error::Error>> {
    logger_init();

    let config = Config::default();
    let addr = config.blockchain_addr.replace("http://", "").parse()?;
    let blockchain = MyBlockchain::new();

    log::info!("Blockchain server listening on {}", addr);

    Server::builder()
        .add_service(blockchain.into_server())
        .serve(addr)
        .await?;

    Ok(())
}

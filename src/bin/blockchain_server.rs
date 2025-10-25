use blockchain_sim::MyBlockchain;
use common::Config;
use tonic::transport::Server;

#[tokio::main]
pub async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    let config = Config::default();
    let addr = config.blockchain_addr.parse()?;
    let blockchain = MyBlockchain::new();

    log::info!("Blockchain server listening on {}", addr);

    Server::builder()
        .add_service(blockchain.into_server())
        .serve(addr)
        .await?;

    Ok(())
}

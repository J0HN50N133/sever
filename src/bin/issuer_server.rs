use common::{Config, logger_init};
use issuer::MyIssuer;
use tonic::transport::Server;

#[tokio::main]
pub async fn main() -> Result<(), Box<dyn std::error::Error>> {
    logger_init();

    let config = Config::default();
    let addr = config.issuer_addr.replace("http://", "").parse()?;
    let issuer = MyIssuer::new(config).await?;

    log::info!("Issuer server listening on {}", addr);

    Server::builder()
        .add_service(issuer.into_server())
        .serve(addr)
        .await?;

    Ok(())
}

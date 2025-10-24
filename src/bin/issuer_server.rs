use issuer::MyIssuer;
use common::revocation::issuer_service_server::IssuerServiceServer;
use common::Config;
use tonic::transport::Server;

#[tokio::main]
pub async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    let config = Config::default();
    let addr = config.issuer_addr.parse()?;
    let issuer = MyIssuer::new(config).await?;

    log::info!("Issuer server listening on {}", addr);

    Server::builder()
        .add_service(issuer.into_server())
        .serve(addr)
        .await?;

    Ok(())
}

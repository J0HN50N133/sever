use log::{debug, warn};
use minchash::MultisetHash as _;
use parking_lot::Mutex;
use rand::{Rng, SeedableRng};
use std::{sync::Arc, time::Duration};
use tokio::time::sleep;

use client_lib::verify_credential;
use common::{
    Config, Credential, generate_did, generate_platform_id, logger_init,
    revocation::{
        IssueRequest, RevokeRequest, blockchain_service_client::BlockchainServiceClient,
        issuer_service_client::IssuerServiceClient,
    },
};

#[tokio::main(flavor = "multi_thread", worker_threads = 10)]
pub async fn main() -> Result<(), Box<dyn std::error::Error>> {
    logger_init();
    let config = Config::default();

    log::info!("Starting simulation with config: {:?}", config);

    // Create gRPC clients
    let _blockchain_client =
        BlockchainServiceClient::connect(config.blockchain_addr.clone()).await?;
    let mut issuer_client = loop {
        if let Ok(cli) = IssuerServiceClient::connect(config.issuer_addr.clone()).await {
            break cli;
        }
        warn!("Waiting for issuer service to be available...");
    };
    debug!("connected to issuer service.");

    let shared_credentials = Arc::new(Mutex::new(Vec::<Credential>::new()));

    // --- Issuance Phase ---
    log::info!("Issuance Phase: Issuing {} credentials.", config.num_users);
    for i in 0..config.num_users {
        let user_did = generate_did();
        let platform_id = generate_platform_id(config.num_platforms);
        let request = tonic::Request::new(IssueRequest {
            user_did: user_did.clone(),
            platform_id: platform_id.clone(),
        });

        match issuer_client.issue_credential(request).await {
            Ok(response) => {
                let proto_cred = response.into_inner();
                let credential = Credential {
                    id: proto_cred.id,
                    user_did: proto_cred.user_did,
                    platform_id: proto_cred.platform_id,
                    witness: minchash::SecureMultisetHash::from_compressed(&proto_cred.witness[..]),
                    version: proto_cred.version,
                    issuer_signature: proto_cred.issuer_signature,
                };
                shared_credentials.lock().push(credential);
                if i % 100 == 0 {
                    log::info!("Issued {} credentials.", i);
                }
            }
            Err(e) => {
                log::error!("Failed to issue credential for user {}: {:?}", user_did, e);
            }
        }
    }
    log::info!(
        "Issuance Phase Complete. Total credentials: {}",
        shared_credentials.lock().len()
    );

    // Give some time for the issuer to process the initial batch
    sleep(Duration::from_secs(config.batch_timeout_ms / 1000 * 2)).await;

    // --- Simulation Loop ---
    log::info!(
        "Starting Simulation Loop for {} seconds.",
        config.simulation_duration_secs
    );
    let _start_time = tokio::time::Instant::now();
    let mut tasks = Vec::new();

    for _ in 0..config.num_users {
        let mut bc_client =
            BlockchainServiceClient::connect(config.blockchain_addr.clone()).await?;
        let mut iss_client = IssuerServiceClient::connect(config.issuer_addr.clone()).await?;
        let creds = shared_credentials.clone();
        let cfg = Arc::new(config.clone());

        tasks.push(tokio::spawn(async move {
            let mut local_creds = creds.lock().clone(); // Each task gets a copy of initial credentials
            if local_creds.is_empty() { return; }

            let mut rng = rand::rngs::StdRng::from_rng(&mut rand::rng());
            let mut simulationDuration = Box::pin(tokio::time::sleep(Duration::from_secs(cfg.simulation_duration_secs)));
            loop {
                tokio::select! {
                    _ = sleep(Duration::from_millis(rng.random_range(100..1000))) => {
                        // Randomly pick an action
                        let action_type = rng.random_range(0..=4);

                        if action_type == 0{ // Simulate new issuance
                            debug!("creating a new cred");
                            let user_did = generate_did();
                            let platform_id = generate_platform_id(cfg.num_platforms);
                            let request = tonic::Request::new(IssueRequest {
                                user_did: user_did.clone(),
                                platform_id: platform_id.clone(),
                            });
                            match iss_client.issue_credential(request).await {
                                Ok(response) => {
                                    let proto_cred = response.into_inner();
                                    let credential = Credential {
                                        id: proto_cred.id,
                                        user_did: proto_cred.user_did,
                                        platform_id: proto_cred.platform_id,
                                        witness: minchash::SecureMultisetHash::from_compressed(&proto_cred.witness[..]),
                                        version: proto_cred.version,
                                        issuer_signature: proto_cred.issuer_signature,
                                    };
                                    creds.lock().push(credential.clone()); // Add to shared list
                                    local_creds.push(credential); // Add to local list for this user
                                },
                                Err(e) => log::error!("Simulation: Failed to issue credential: {:?}", e),
                            }
                        } else if action_type <= 3 && !local_creds.is_empty() { // Simulate revocation
                            // revoke a random credential from local_creds
                            let idx = rng.random_range(0..local_creds.len());
                            let cred_to_revoke = local_creds.remove(idx);
                            let request = tonic::Request::new(RevokeRequest { credential_id: cred_to_revoke.id.clone() });
                            match iss_client.revoke_credential(request).await {
                                Ok(_) => log::info!("Simulation: Revoked credential {}.", cred_to_revoke.id),
                                Err(e) => log::error!("Simulation: Failed to revoke credential {}: {:?}", cred_to_revoke.id, e),
                            }
                        } else if !local_creds.is_empty() { // Simulate verification
                            let idx = rng.random_range(0..local_creds.len());
                            let mut cred_to_verify = local_creds[idx].clone();
                            match verify_credential(&mut cred_to_verify, &mut bc_client, &mut iss_client).await {
                                Ok(true) => log::debug!("Simulation: Verified credential {}.", cred_to_verify.id),
                                Ok(false) => log::warn!("Simulation: Failed to verify credential {}.", cred_to_verify.id),
                                Err(e) => log::error!("Simulation: Error during verification of {}: {:?}", cred_to_verify.id, e),
                            }
                            local_creds[idx] = cred_to_verify; // Update local credential if witness was updated
                        }
                    }
                    _ = &mut simulationDuration => {
                        // Exit task after simulation duration
                        break;
                    }
                }
            }
        }));
    }

    // Wait for all simulation tasks to complete
    for task in tasks {
        task.await?;
    }

    log::info!("Simulation finished.");

    // --- Metrics Collection & Reporting (Placeholder for now) ---
    log::info!("Metrics collection and reporting would go here.");
    log::info!(
        "Total credentials in system at end: {}",
        shared_credentials.lock().len()
    );

    Ok(())
}

use async_trait::async_trait;
use log::warn;
use std::{collections::HashMap, sync::Arc, time::Duration};
use tokio::sync::Mutex;
use tokio::time::sleep;
use tonic::{Request, Response, Status};

use common::{
    generate_credential_id,
    revocation::{
        blockchain_service_client::BlockchainServiceClient,
        issuer_service_server::{IssuerService, IssuerServiceServer},
        Credential as ProtoCredential, IssueRequest, Proof, ProofRequest, RevokeRequest,
        UpdateAccumulatorRequest,
    },
    Config, Credential,
};
use minchash::{MultisetHash, SecureMultisetHash};

#[derive(Clone)]
pub struct MyIssuer {
    state: Arc<Mutex<IssuerState>>,
    _config: Arc<Config>,
    blockchain_client: Arc<Mutex<BlockchainServiceClient<tonic::transport::Channel>>>,
}

#[derive(Debug)]
pub struct IssuerState {
    pub accumulator: SecureMultisetHash,
    pub credentials_db: HashMap<String, Credential>, // Stores full credential data
    pub current_version: u64,
}

impl MyIssuer {
    pub async fn new(config: Config) -> anyhow::Result<Self> {
        let blockchain_client = loop {
            if let Ok(cli) = BlockchainServiceClient::connect(config.blockchain_addr.clone()).await
            {
                break cli;
            }
            warn!("Waiting for blockchain service to be available...");
            sleep(Duration::from_millis(500)).await;
        };

        let state = Arc::new(Mutex::new(IssuerState {
            accumulator: SecureMultisetHash::new(),
            credentials_db: HashMap::new(),
            current_version: 0,
        }));

        let issuer = MyIssuer {
            state,
            _config: Arc::new(config),
            blockchain_client: Arc::new(Mutex::new(blockchain_client)),
        };

        Ok(issuer)
    }

    pub fn into_server(self) -> IssuerServiceServer<MyIssuer> {
        IssuerServiceServer::new(self)
    }
}

#[async_trait]
impl IssuerService for MyIssuer {
    async fn issue_credential(
        &self,
        request: Request<IssueRequest>,
    ) -> Result<Response<ProtoCredential>, Status> {
        let req = request.into_inner();
        let credential_id = generate_credential_id(&req.user_did, &req.platform_id);
        let credential_hash = blake3::hash(credential_id.as_bytes()).as_bytes().to_vec();

        log::info!(
            "Starting to issue credential: {} for user: {}, platform: {}",
            credential_id,
            req.user_did,
            req.platform_id
        );

        let mut state = self.state.lock().await;

        // Check if credential already exists
        if state.credentials_db.contains_key(&credential_id) {
            log::warn!("Credential {} already exists in database", credential_id);
            return Err(Status::invalid_argument("Credential already exists"));
        }

        let credential = Credential {
            id: credential_id.clone(),
            user_did: req.user_did.clone(),
            platform_id: req.platform_id.clone(),
            witness: SecureMultisetHash::new(), // Placeholder
            version: state.current_version,
            issuer_signature: Vec::new(), // Placeholder
        };

        // Atomic operation: add to accumulator and DB
        let original_accumulator = state.accumulator.clone();
        let original_version = state.current_version;

        log::debug!(
            "Adding credential hash to accumulator for {}",
            credential_id
        );
        state.accumulator.add_elements(&[credential_hash.clone()]);

        if let Some(new_root_hash) = state.accumulator.get_digest() {
            state.current_version += 1;
            state
                .credentials_db
                .insert(credential_id.clone(), credential.clone());

            log::debug!(
                "Credential {} added to database, new version: {}",
                credential_id,
                state.current_version
            );

            let new_version = state.current_version;
            let update_req = UpdateAccumulatorRequest {
                new_root_hash,
                new_version,
            };

            // Drop lock before blockchain call
            drop(state);

            log::debug!("Connecting to blockchain to update accumulator");
            let mut client = self.blockchain_client.lock().await;
            log::info!(
                "Updating blockchain with new root hash (version {}).",
                new_version
            );

            match client.update_accumulator(update_req).await {
                Ok(_) => {
                    log::info!(
                        "Blockchain updated successfully to version {}.",
                        new_version
                    );

                    let proto_cred = ProtoCredential {
                        id: credential.id,
                        user_did: credential.user_did,
                        platform_id: credential.platform_id,
                        witness: credential.witness.get_digest().unwrap_or_default(),
                        version: new_version,
                        issuer_signature: credential.issuer_signature,
                    };

                    log::info!(
                        "Successfully issued credential {}. State updated to version {}.",
                        proto_cred.id,
                        new_version
                    );
                    Ok(Response::new(proto_cred))
                }
                Err(e) => {
                    log::error!("Failed to update blockchain: {:?}. Reverting state.", e);
                    // Re-acquire lock to revert state
                    let mut state = self.state.lock().await;
                    log::warn!(
                        "Reverting accumulator and database changes for credential {}",
                        credential_id
                    );
                    state.accumulator = original_accumulator;
                    state.current_version = original_version;
                    state.credentials_db.remove(&credential_id);
                    Err(Status::internal(format!(
                        "Failed to update blockchain: {}",
                        e
                    )))
                }
            }
        } else {
            log::warn!("Accumulator digest is empty, not updating blockchain.");
            Err(Status::internal("Failed to update accumulator"))
        }
    }

    async fn revoke_credential(
        &self,
        request: Request<RevokeRequest>,
    ) -> Result<Response<()>, Status> {
        let req = request.into_inner();
        let credential_id = req.credential_id;

        let credential_hash = blake3::hash(credential_id.as_bytes()).as_bytes().to_vec();

        log::info!("Starting to revoke credential: {}", credential_id);

        let mut state = self.state.lock().await;

        // Check if credential exists and remove it atomically
        let original_credential = state.credentials_db.remove(&credential_id).ok_or_else(|| {
            log::warn!(
                "Attempted to revoke non-existent credential: {}",
                credential_id
            );
            Status::not_found("Credential not found")
        })?;

        log::debug!(
            "Credential {} found in database, proceeding with revocation",
            credential_id
        );

        // Atomic operation: remove from accumulator
        let original_accumulator = state.accumulator.clone();
        let original_version = state.current_version;

        log::debug!(
            "Removing credential hash from accumulator for {}",
            credential_id
        );
        state
            .accumulator
            .remove_elements(&[credential_hash.clone()]);

        if let Some(new_root_hash) = state.accumulator.get_digest() {
            state.current_version += 1;

            log::debug!(
                "Credential {} removed from accumulator, new version: {}",
                credential_id,
                state.current_version
            );

            let new_version = state.current_version;
            let update_req = UpdateAccumulatorRequest {
                new_root_hash,
                new_version,
            };

            // Drop lock before blockchain call
            drop(state);

            log::debug!("Connecting to blockchain to update accumulator after revocation");
            let mut client = self.blockchain_client.lock().await;
            log::info!(
                "Updating blockchain with new root hash (version {}).",
                new_version
            );

            match client.update_accumulator(update_req).await {
                Ok(_) => {
                    log::info!(
                        "Blockchain updated successfully to version {}.",
                        new_version
                    );
                    log::info!("Successfully revoked credential {}.", credential_id);
                    Ok(Response::new(()))
                }
                Err(e) => {
                    log::error!("Failed to update blockchain: {:?}. Reverting state.", e);
                    // Re-acquire lock to revert state
                    let mut state = self.state.lock().await;
                    log::warn!(
                        "Reverting accumulator and database changes for credential {}",
                        credential_id
                    );
                    state.accumulator = original_accumulator;
                    state.current_version = original_version;
                    state
                        .credentials_db
                        .insert(original_credential.id.clone(), original_credential);
                    Err(Status::internal(format!(
                        "Failed to update blockchain: {}",
                        e
                    )))
                }
            }
        } else {
            log::warn!(
                "Accumulator digest is empty, reverting DB change for credential {}",
                credential_id
            );
            // Revert DB change if accumulator update failed
            state
                .credentials_db
                .insert(original_credential.id.clone(), original_credential);
            Err(Status::internal("Failed to update accumulator"))
        }
    }

    // NOTE:  accmulator里如果只有一个元素, 返回的witness是空
    async fn request_proof(
        &self,
        request: Request<ProofRequest>,
    ) -> Result<Response<Proof>, Status> {
        let req = request.into_inner();
        let credential_id = req.credential_id;

        log::info!("Requesting proof for credential: {}", credential_id);

        let s = self.state.lock().await;
        if let Some(_credential) = s.credentials_db.get(&credential_id) {
            log::debug!("Credential {} found in database", credential_id);
            let credential_hash = blake3::hash(credential_id.as_bytes()).as_bytes().to_vec();

            if let Some(witness) = s.accumulator.generate_proof(&credential_hash) {
                let reply = Proof {
                    witness: witness.get_digest().unwrap_or_default(),
                    version: s.current_version,
                };
                log::info!(
                    "Successfully generated proof for credential {} at version {}. Credential status: ACTIVE",
                    credential_id,
                    s.current_version
                );
                log::debug!(
                    "Proof details - Credential: {}, Version: {}, Witness length: {}",
                    credential_id,
                    s.current_version,
                    reply.witness.len()
                );
                Ok(Response::new(reply))
            } else {
                log::error!("Failed to generate proof for credential {}. This indicates a consistency error - credential exists in DB but not in accumulator!", credential_id);
                Err(Status::internal(
                    "Failed to generate proof - credential not found in accumulator",
                ))
            }
        } else {
            log::warn!(
                "Attempted to request proof for non-existent credential: {}. Credential status: NOT_FOUND",
                credential_id
            );
            Err(Status::not_found("Credential not found"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use common::revocation::blockchain_service_client::BlockchainServiceClient;
    use tokio_stream::wrappers::TcpListenerStream;
    use tonic::transport::Server;
    use tonic::Request; // Import Request explicitly

    #[tokio::test]
    async fn test_issue_and_remove() {
        logforth::starter_log::stdout().apply();
        // 1. Start the blockchain_sim server on an ephemeral port
        let blockchain_service = blockchain_sim::MyBlockchain::new();
        let blockchain_server = blockchain_service.into_server();

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let blockchain_addr = format!("http://{}", addr);

        tokio::spawn(async move {
            Server::builder()
                .add_service(blockchain_server)
                .serve_with_incoming(TcpListenerStream::new(listener))
                .await
                .unwrap();
        });

        // 2. Create Config for Issuer
        let config = Config {
            blockchain_addr: blockchain_addr.clone(),
            ..Default::default() // Fill other config fields with defaults if any
        };

        // 3. Construct MyIssuer using MyIssuer::new()
        let issuer = MyIssuer::new(config).await.unwrap();

        // 4. Create a BlockchainServiceClient to query the blockchain state for assertions
        let mut blockchain_client = BlockchainServiceClient::connect(blockchain_addr)
            .await
            .unwrap();

        // 5. Issue multiple credentials to avoid empty accumulator issue
        let mut credentials = Vec::new();
        let test_cases = vec![
            ("did:user:123", "platform:abc"),
            ("did:user:456", "platform:def"),
            ("did:user:789", "platform:ghi"),
            ("did:user:999", "platform:xyz"),
        ];

        for (user_did, platform_id) in test_cases {
            let issue_req = IssueRequest {
                user_did: user_did.to_string(),
                platform_id: platform_id.to_string(),
            };
            let res = issuer
                .issue_credential(Request::new(issue_req))
                .await
                .unwrap();
            let credential = res.into_inner();
            log::debug!("Issued credential: {:?}", credential);
            credentials.push(credential);
        }

        let credential = &credentials[0]; // Use first credential for proof tests

        assert_eq!(credential.user_did, "did:user:123");
        assert_eq!(credential.version, 1);

        // 6. Verify issuer and blockchain state after issuing all credentials
        {
            let issuer_state = issuer.state.lock().await;
            assert_eq!(issuer_state.current_version, 4); // Should be 4 after issuing 4 credentials
            assert_eq!(issuer_state.credentials_db.len(), 4);
            assert!(issuer_state.credentials_db.contains_key(&credential.id));

            // Verify blockchain state via gRPC call
            let blockchain_acc_state = blockchain_client
                .get_accumulator(Request::new(())) // Fixed: Request::new(())
                .await
                .unwrap()
                .into_inner();
            assert_eq!(blockchain_acc_state.version, 4);
            assert_eq!(
                blockchain_acc_state.root_hash,
                issuer_state.accumulator.get_digest().unwrap()
            );
        }

        // 7. Request a proof for the issued credential
        let proof_req = ProofRequest {
            credential_id: credential.id.clone(),
        };
        let proof_res = issuer.request_proof(Request::new(proof_req)).await.unwrap();
        let proof = proof_res.into_inner();
        assert_eq!(proof.version, 4); // Should be 4 after issuing 4 credentials

        // Verify the proof (now should work with multiple credentials in accumulator)
        assert!(!proof.witness.is_empty()); // Fixed: Proof should have non-empty witness with multiple credentials

        // 8. Revoke the credential
        let revoke_req = RevokeRequest {
            credential_id: credential.id.clone(),
        };
        issuer
            .revoke_credential(Request::new(revoke_req))
            .await
            .unwrap();

        // 9. Verify state after revocation
        {
            let issuer_state = issuer.state.lock().await;
            assert_eq!(issuer_state.current_version, 5); // Should be 5 after revocation (4 + 1)
            assert_eq!(issuer_state.credentials_db.len(), 3); // Should be 3 remaining credentials

            // Verify blockchain state via gRPC call
            let blockchain_acc_state = blockchain_client
                .get_accumulator(Request::new(())) // Fixed: Request::new(())
                .await
                .unwrap()
                .into_inner();
            assert_eq!(blockchain_acc_state.version, 5);
            assert_eq!(
                blockchain_acc_state.root_hash,
                issuer_state.accumulator.get_digest().unwrap()
            );
        }

        // 10. Requesting proof for revoked credential should fail
        let proof_req_revoked = ProofRequest {
            credential_id: credential.id.clone(),
        };
        let proof_err = issuer
            .request_proof(Request::new(proof_req_revoked))
            .await
            .unwrap_err();
        assert_eq!(proof_err.code(), tonic::Code::NotFound);

        // 11. Revoking again should fail
        let revoke_req_again = RevokeRequest {
            credential_id: credential.id.clone(),
        };
        let revoke_err = issuer
            .revoke_credential(Request::new(revoke_req_again))
            .await
            .unwrap_err();
        assert_eq!(revoke_err.code(), tonic::Code::NotFound);
    }
}

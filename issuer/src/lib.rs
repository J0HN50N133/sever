use async_trait::async_trait;
use blake3;
use futures::StreamExt;
use parking_lot::Mutex;
use std::{collections::HashMap, sync::Arc, time::Duration};
use tokio::time::sleep;
use tonic::{Request, Response, Status};

use common::{
    generate_credential_id,
    revocation::{
        blockchain_service_client::BlockchainServiceClient,
        issuer_service_server::{IssuerService, IssuerServiceServer},
        AccumulatorState, Credential as ProtoCredential, IssueRequest, Proof, ProofRequest,
        RevokeRequest, UpdateAccumulatorRequest,
    },
    Config, Credential,
};
use minchash::{MultisetHash, SecureMultisetHash};

#[derive(Clone)]
pub struct MyIssuer {
    state: Arc<Mutex<IssuerState>>,
    config: Arc<Config>,
}

pub struct IssuerState {
    pub accumulator: SecureMultisetHash,
    pub credentials_db: HashMap<String, Credential>, // Stores full credential data
    pub pending_requests: Vec<IssuerRequest>,
    pub current_version: u64,
}

impl std::fmt::Debug for IssuerState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IssuerState")
            .field("credentials_db", &self.credentials_db)
            .field("pending_requests", &self.pending_requests)
            .field("current_version", &self.current_version)
            .finish()
    }
}

#[derive(Debug, Clone)]
pub enum IssuerRequest {
    Add(String, Vec<u8>),    // credential_id, credential_hash
    Remove(String, Vec<u8>), // credential_id, credential_hash
}

impl MyIssuer {
    pub async fn new(config: Config) -> Result<Self, Box<dyn std::error::Error>> {
        let blockchain_client =
            BlockchainServiceClient::connect(config.blockchain_addr.clone()).await?;

        let state = Arc::new(Mutex::new(IssuerState {
            accumulator: SecureMultisetHash::new(),
            credentials_db: HashMap::new(),
            pending_requests: Vec::new(),
            current_version: 0,
        }));

        let issuer = MyIssuer {
            state: state.clone(),
            config: Arc::new(config),
        };

        // Start batch processing task
        tokio::spawn(issuer.clone().batch_processor(blockchain_client));

        Ok(issuer)
    }

    pub fn into_server(self) -> IssuerServiceServer<MyIssuer> {
        IssuerServiceServer::new(self)
    }

    async fn batch_processor(
        self,
        mut blockchain_client: BlockchainServiceClient<tonic::transport::Channel>,
    ) {
        let config = self.config.clone();
        let state = self.state.clone();
        let batch_timeout = Duration::from_millis(config.batch_timeout_ms);

        loop {
            sleep(batch_timeout).await;

            let (new_root_hash_val, new_version_val) = {
                let mut s = state.lock();
                let pending_requests = std::mem::take(&mut s.pending_requests);

                if pending_requests.is_empty() {
                    continue;
                }

                log::info!("Processing batch of {} requests.", pending_requests.len());

                let mut elements_to_add = Vec::new();
                let mut elements_to_remove = Vec::new();

                for req in pending_requests {
                    match req {
                        IssuerRequest::Add(_, cred_hash) => {
                            elements_to_add.push(cred_hash);
                        }
                        IssuerRequest::Remove(_, cred_hash) => {
                            elements_to_remove.push(cred_hash);
                        }
                    }
                }

                if !elements_to_add.is_empty() {
                    s.accumulator.add_elements(&elements_to_add);
                }
                if !elements_to_remove.is_empty() {
                    s.accumulator.remove_elements(&elements_to_remove);
                }

                if let Some(new_root_hash) = s.accumulator.get_digest() {
                    s.current_version += 1;
                    (Some(new_root_hash), s.current_version)
                } else {
                    (None, s.current_version)
                }
            };

            if let Some(new_root_hash_val) = new_root_hash_val {
                log::info!(
                    "Updating blockchain with new root hash (version {}).",
                    new_version_val
                );
                let request = tonic::Request::new(UpdateAccumulatorRequest {
                    new_root_hash: new_root_hash_val,
                    new_version: new_version_val,
                });

                match blockchain_client.update_accumulator(request).await {
                    Ok(_) => log::info!(
                        "Blockchain updated successfully to version {}.",
                        new_version_val
                    ),
                    Err(e) => log::error!("Failed to update blockchain: {:?}", e),
                }
            } else {
                log::warn!("Accumulator digest is empty, not updating blockchain.");
            }
        }
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

        let mut s = self.state.lock();
        let current_version = s.current_version;

        // Create a placeholder credential. Witness and signature will be updated later.
        let credential = Credential {
            id: credential_id.clone(),
            user_did: req.user_did,
            platform_id: req.platform_id,
            witness: SecureMultisetHash::new(), // Changed here
            version: current_version,
            issuer_signature: Vec::new(), // Placeholder
        };

        s.credentials_db
            .insert(credential_id.clone(), credential.clone());
        s.pending_requests
            .push(IssuerRequest::Add(credential_id.clone(), credential_hash));

        // If batch size is 1, process immediately for testing/simplicity
        if self.config.batch_size == 1 {
            drop(s); // Release lock before calling batch_processor
                     // In a real async system, this would signal the batch processor
                     // For simplicity in simulation, we'll let the batch processor's timer handle it.
        }

        // Return a proto credential (without witness/signature yet)
        let proto_cred = ProtoCredential {
            id: credential.id,
            user_did: credential.user_did,
            platform_id: credential.platform_id,
            witness: credential.witness.get_digest().unwrap_or_default(), // Convert SecureMultisetHash to Vec<u8> for proto
            version: credential.version,
            issuer_signature: credential.issuer_signature,
        };

        log::info!(
            "Issued credential {}. Pending batch processing.",
            proto_cred.id
        );
        Ok(Response::new(proto_cred))
    }

    async fn revoke_credential(
        &self,
        request: Request<RevokeRequest>,
    ) -> Result<Response<()>, Status> {
        // Use imported Empty
        let req = request.into_inner();
        let credential_id = req.credential_id;

        let mut s = self.state.lock();
        if let Some(credential) = s.credentials_db.remove(&credential_id) {
            let credential_hash = blake3::hash(credential_id.as_bytes()).as_bytes().to_vec();
            s.pending_requests.push(IssuerRequest::Remove(
                credential_id.clone(),
                credential_hash,
            ));
            log::info!(
                "Revoked credential {}. Pending batch processing.",
                credential_id
            );
            Ok(Response::new(())) // Use imported Empty
        } else {
            log::warn!(
                "Attempted to revoke non-existent credential: {}",
                credential_id
            );
            Err(Status::not_found("Credential not found"))
        }
    }

    async fn request_proof(
        &self,
        request: Request<ProofRequest>,
    ) -> Result<Response<Proof>, Status> {
        let req = request.into_inner();
        let credential_id = req.credential_id;

        let s = self.state.lock();
        if let Some(credential) = s.credentials_db.get(&credential_id) {
            let credential_hash = blake3::hash(credential_id.as_bytes()).as_bytes().to_vec();
            if let Some(witness) = s.accumulator.generate_proof(&credential_hash) {
                let reply = Proof {
                    witness: witness.get_digest().unwrap_or_default(), // Convert SecureMultisetHash to Vec<u8> for proto
                    version: s.current_version,
                };
                log::debug!(
                    "Generated proof for credential {} at version {}. Accumulator: {:?}",
                    credential_id,
                    s.current_version,
                    s.accumulator
                );
                Ok(Response::new(reply))
            } else {
                log::error!("Failed to generate proof for credential {}. Accumulator might be empty or element not present.", credential_id);
                Err(Status::internal("Failed to generate proof"))
            }
        } else {
            log::warn!(
                "Attempted to request proof for non-existent credential: {}",
                credential_id
            );
            Err(Status::not_found("Credential not found"))
        }
    }
}

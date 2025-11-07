use async_trait::async_trait;
use log::warn;
use parking_lot::Mutex;
use std::{collections::HashMap, sync::Arc, time::Duration};
use tokio::sync::watch;
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
    config: Arc<Config>,
    batch_notifier: watch::Sender<()>, // Notify batch processor of new requests
}

#[derive(Debug)]
pub struct IssuerState {
    pub accumulator: SecureMultisetHash,
    pub credentials_db: HashMap<String, Credential>, // Stores full credential data
    pub pending_requests: Vec<IssuerRequest>,
    pub current_version: u64,
    pub batch_start_time: Option<std::time::Instant>, // Track when current batch started
}

#[derive(Debug, Clone)]
pub enum IssuerRequest {
    Add(String, Vec<u8>),    // credential_id, credential_hash
    Remove(String, Vec<u8>), // credential_id, credential_hash
}

impl MyIssuer {
    pub async fn new(config: Config) -> anyhow::Result<Self> {
        let blockchain_client = loop {
            if let Ok(cli) = BlockchainServiceClient::connect(config.blockchain_addr.clone()).await
            {
                break cli;
            }
            warn!("Waiting for blockchain service to be available...");
        };

        let state = Arc::new(Mutex::new(IssuerState {
            accumulator: SecureMultisetHash::new(),
            credentials_db: HashMap::new(),
            pending_requests: Vec::new(),
            current_version: 0,
            batch_start_time: None,
        }));

        let (batch_notifier, _) = watch::channel(());

        let issuer = MyIssuer {
            state: state.clone(),
            config: Arc::new(config),
            batch_notifier: batch_notifier.clone(),
        };

        // Start batch processing task
        tokio::spawn(issuer.clone().batch_processor(blockchain_client));

        Ok(issuer)
    }

    pub fn into_server(self) -> IssuerServiceServer<MyIssuer> {
        IssuerServiceServer::new(self)
    }

    // Check if batch should be processed based on size or timeout
    fn should_process_batch(
        &self,
        pending_count: usize,
        batch_start_time: Option<std::time::Instant>,
    ) -> bool {
        let batch_size_reached = pending_count >= self.config.batch_size as usize;

        if let Some(start_time) = batch_start_time {
            let timeout_reached =
                start_time.elapsed() >= Duration::from_millis(self.config.batch_timeout_ms);
            batch_size_reached || timeout_reached
        } else {
            batch_size_reached
        }
    }

    // Process pending requests immediately
    async fn process_pending_requests(
        &self,
        blockchain_client: &mut BlockchainServiceClient<tonic::transport::Channel>,
    ) {
        let (new_root_hash_val, new_version_val) = {
            let mut s = self.state.lock();
            let pending_requests = std::mem::take(&mut s.pending_requests);
            s.batch_start_time = None; // Reset batch timer

            if pending_requests.is_empty() {
                return;
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

    async fn batch_processor(
        self,
        mut blockchain_client: BlockchainServiceClient<tonic::transport::Channel>,
    ) {
        let state = self.state.clone();
        let mut batch_rx = self.batch_notifier.subscribe();
        let batch_timeout = Duration::from_millis(self.config.batch_timeout_ms);

        loop {
            // Check if there are pending requests
            let (pending_count, batch_start_time, has_pending) = {
                let s = state.lock();
                (
                    s.pending_requests.len(),
                    s.batch_start_time,
                    !s.pending_requests.is_empty(),
                )
            };

            if !has_pending {
                // No pending requests, wait for notification
                let _ = batch_rx.changed().await;
                continue;
            }

            // Check if we should process immediately due to batch size
            if self.should_process_batch(pending_count, batch_start_time) {
                self.process_pending_requests(&mut blockchain_client).await;
                continue;
            }

            // Wait for either timeout or new request notification
            let timeout_future = async {
                if let Some(start_time) = batch_start_time {
                    let elapsed = start_time.elapsed();
                    if elapsed < batch_timeout {
                        sleep(batch_timeout - elapsed).await;
                    }
                } else {
                    sleep(batch_timeout).await;
                }
            };

            tokio::select! {
                _ = timeout_future => {
                    // Timeout reached, process the batch
                    self.process_pending_requests(&mut blockchain_client).await;
                }
                _ = batch_rx.changed() => {
                    // New request arrived, loop again to check batch size
                    continue;
                }
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

        // Start batch timer if this is the first request in the batch
        if s.batch_start_time.is_none() {
            s.batch_start_time = Some(std::time::Instant::now());
        }

        let pending_count = s.pending_requests.len();
        let batch_start_time = s.batch_start_time;
        drop(s); // Release lock

        // Notify batch processor that there's a new request
        let _ = self.batch_notifier.send(());

        // Check if batch should be processed immediately due to size
        if self.should_process_batch(pending_count, batch_start_time) {
            log::info!("Batch processing triggered by reaching batch size in issue_credential");
            // The batch processor will be awakened and check the condition
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
        if let Some(_credential) = s.credentials_db.remove(&credential_id) {
            let credential_hash = blake3::hash(credential_id.as_bytes()).as_bytes().to_vec();
            s.pending_requests.push(IssuerRequest::Remove(
                credential_id.clone(),
                credential_hash,
            ));

            // Start batch timer if this is the first request in the batch
            if s.batch_start_time.is_none() {
                s.batch_start_time = Some(std::time::Instant::now());
            }

            let pending_count = s.pending_requests.len();
            let batch_start_time = s.batch_start_time;
            drop(s); // Release lock

            log::info!(
                "Revoked credential {}. Pending batch processing.",
                credential_id
            );

            // Notify batch processor that there's a new request
            let _ = self.batch_notifier.send(());

            // Check if batch should be processed immediately due to size
            if self.should_process_batch(pending_count, batch_start_time) {
                log::info!(
                    "Batch processing triggered by reaching batch size in revoke_credential"
                );
                // The batch processor will be awakened and check the condition
            }

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
        if let Some(_credential) = s.credentials_db.get(&credential_id) {
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

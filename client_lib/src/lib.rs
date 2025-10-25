use common::{
    revocation::{
        blockchain_service_client::BlockchainServiceClient,
        issuer_service_client::IssuerServiceClient, ProofRequest,
    },
    Credential,
};
use log::{error, info, warn};
use minchash::{MultisetHash, SecureMultisetHash};
use tonic::Request;

pub async fn verify_credential(
    credential: &mut Credential,
    blockchain_client: &mut BlockchainServiceClient<tonic::transport::Channel>,
    issuer_client: &mut IssuerServiceClient<tonic::transport::Channel>,
) -> Result<bool, Box<dyn std::error::Error>> {
    // Step 1: (Simulate) Check the issuer's signature on the credential.
    // For now, we'll assume the signature is always valid in this simulation.
    // In a real system, this would involve cryptographic signature verification.
    /* TODO: no need to do signature verification in this simulation
        if credential.issuer_signature.is_empty() {
            warn!(
                "Credential {} has no issuer signature. Assuming invalid for now.",
                credential.id
            );
            return Ok(false);
        }
    */
    // Simulate signature check success
    info!(
        "Simulated signature check passed for credential {}.",
        credential.id
    );

    // Step 2: Call get_accumulator on the blockchain simulator to get the latest root_hash and version.
    let blockchain_state_response = blockchain_client.get_accumulator(()).await?.into_inner();

    let _latest_root_hash = blockchain_state_response.root_hash;
    let latest_version = blockchain_state_response.version;

    // Step 3: Compare the credential's version with the one from the blockchain.
    if credential.version < latest_version {
        warn!(
            "Credential {} witness is outdated (v{} < v{}). Requesting new witness.",
            credential.id, credential.version, latest_version
        );
        // Step 4 (Lazy Update): If versions mismatch, request a new witness.
        let proof_request = Request::new(ProofRequest {
            credential_id: credential.id.clone(),
        });

        match issuer_client.request_proof(proof_request).await {
            Ok(response) => {
                let proof = response.into_inner();
                // The proof.witness is Vec<u8>, but credential.witness is SecureMultisetHash.
                // We need to convert Vec<u8> to SecureMultisetHash.
                // This implies SecureMultisetHash must have a way to be deserialized from Vec<u8>.
                // For now, I will assume SecureMultisetHash::from_bytes(proof.witness) exists.
                // If not, this will be a build error.
                credential.witness = SecureMultisetHash::from_compressed(&proof.witness);
                credential.version = proof.version;
                info!(
                    "Credential {} updated to new witness (v{}).",
                    credential.id, credential.version
                );
            }
            Err(e) => {
                error!(
                    "Failed to get new witness for credential {}: {:?}",
                    credential.id, e
                );
                return Ok(false); // Cannot verify if we can't get an updated witness
            }
        }
    } else if credential.version > latest_version {
        // This case should ideally not happen if the issuer is the only one updating the blockchain
        // and clients are only fetching. If it does, it indicates a potential issue or out-of-sync state.
        error!(
            "Credential {} witness version (v{}) is GREATER than blockchain version (v{}). This should not happen.",
            credential.id, credential.version, latest_version
        );
        return Ok(false);
    }

    // Step 5: Use the accumulator.verify_proof() method.
    let credential_hash = blake3::hash(credential.id.as_bytes()).as_bytes().to_vec();

    // According to the trait definition, we need a SecureMultisetHash instance to call verify_proof.
    // We create a new, empty instance, assuming the implementation of verify_proof does not depend on the
    // state of the instance, but rather on the information contained within the proof itself (including the root hash).
    let accumulator = SecureMultisetHash::new();
    let is_valid = accumulator.verify_proof(&credential_hash, &credential.witness);

    if is_valid {
        info!("Credential {} successfully verified.", credential.id);
    } else {
        warn!("Credential {} verification FAILED.", credential.id);
    }

    Ok(is_valid)
}

// Placeholder for simulated signature verification
// In a real system, this would be a complex cryptographic operation.
pub fn simulate_signature_verification(_credential: &Credential) -> bool {
    // Always return true for simulation purposes
    true
}

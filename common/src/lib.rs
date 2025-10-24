use std::fmt;

use minchash::{MultisetHash, SecureMultisetHash};
pub use proto_gen::revocation;
use rand::Rng;
use serde::de::Visitor;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Config {
    pub blockchain_addr: String,
    pub issuer_addr: String,
    pub batch_size: u32,
    pub batch_timeout_ms: u64,
    pub num_users: usize,
    pub num_platforms: usize,
    pub issue_req_per_sec: f64,
    pub revoke_req_per_sec: f64,
    pub proof_req_per_sec: f64,
    pub simulation_duration_secs: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            blockchain_addr: "http://[::1]:50051".to_string(),
            issuer_addr: "http://[::1]:50052".to_string(),
            batch_size: 100,
            batch_timeout_ms: 1000, // 1 second
            num_users: 1000,
            num_platforms: 5,
            issue_req_per_sec: 10.0,
            revoke_req_per_sec: 1.0,
            proof_req_per_sec: 100.0,
            simulation_duration_secs: 60,
        }
    }
}

fn serialize_witness<S>(witness: &SecureMultisetHash, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    // 先压缩成 Vec<u8>
    let bytes = witness
        .get_compressed()
        .ok_or_else(|| serde::ser::Error::custom("failed to compress SecureMultisetHash"))?;
    serializer.serialize_bytes(&bytes)
}

fn deserialize_witness<'de, D>(deserializer: D) -> Result<SecureMultisetHash, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct BytesVisitor;

    impl<'de> Visitor<'de> for BytesVisitor {
        type Value = Vec<u8>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a byte array representing SecureMultisetHash")
        }

        fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            Ok(v.to_vec())
        }

        fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
        where
            A: serde::de::SeqAccess<'de>,
        {
            let mut vec = Vec::new();
            while let Some(byte) = seq.next_element()? {
                vec.push(byte);
            }
            Ok(vec)
        }
    }

    let bytes = deserializer.deserialize_bytes(BytesVisitor)?;
    Ok(SecureMultisetHash::from_compressed(&bytes[..]))
}

// Credential struct for internal use, can be converted from/to proto::Credential
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Credential {
    pub id: String, // Unique ID for the credential
    pub user_did: String,
    pub platform_id: String,
    #[serde(
        serialize_with = "serialize_witness",
        deserialize_with = "deserialize_witness"
    )]
    pub witness: SecureMultisetHash,
    pub version: u64,
    pub issuer_signature: Vec<u8>,
}

// Helper function to generate a unique credential ID
pub fn generate_credential_id(user_did: &str, platform_id: &str) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(user_did.as_bytes());
    hasher.update(platform_id.as_bytes());
    hasher.finalize().to_hex().to_string()
}

// Helper function to generate a random DID
pub fn generate_did() -> String {
    let mut rng = rand::rng();
    format!("did:example:{}", rng.random::<u64>())
}

// Helper function to generate a random platform ID
pub fn generate_platform_id(num_platforms: usize) -> String {
    let mut rng = rand::rng();
    format!("platform:{}", rng.random_range(0..num_platforms))
}

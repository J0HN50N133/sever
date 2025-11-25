use minchash::{MultisetHash, SecureMultisetHash};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::time::Duration;

/// Represents the blockchain storing accumulator roots with version numbers
#[derive(Debug, Clone)]
pub struct Blockchain {
    roots: HashMap<u64, Vec<u8>>, // version -> accumulator root
    current_version: u64,
}

impl Blockchain {
    pub fn new() -> Self {
        Blockchain {
            roots: HashMap::new(),
            current_version: 0,
        }
    }

    pub fn store_root(&mut self, version: u64, root: Vec<u8>) {
        self.roots.insert(version, root);
        if version > self.current_version {
            self.current_version = version;
        }
    }

    pub fn get_root(&self, version: u64) -> Option<&Vec<u8>> {
        self.roots.get(&version)
    }

    pub fn get_current_version(&self) -> u64 {
        self.current_version
    }

    pub fn get_current_root(&self) -> Option<&Vec<u8>> {
        self.roots.get(&self.current_version)
    }
}

/// Represents the credential issuer.
#[derive(Clone)]
pub struct Issuer {
    pub accumulator: SecureMultisetHash,
    current_version: u64,
    blockchain: Blockchain,
}

impl Issuer {
    pub fn new() -> Self {
        Issuer {
            accumulator: SecureMultisetHash::new(),
            current_version: 1, // Start with version 1
            blockchain: Blockchain::new(),
        }
    }

    pub fn get_current_version(&self) -> u64 {
        self.current_version
    }

    pub fn get_blockchain(&self) -> &Blockchain {
        &self.blockchain
    }

    /// Adds elements to the accumulator and updates blockchain version.
    pub fn add_elements(&mut self, elements: &[&[u8]]) {
        self.accumulator.add_elements(elements);
        self.increment_version_and_store();
    }

    /// Removes elements from the accumulator and updates blockchain version.
    pub fn remove_elements(&mut self, elements: &[&[u8]]) {
        self.accumulator.remove_elements(elements);
        self.increment_version_and_store();
    }

    fn increment_version_and_store(&mut self) {
        self.current_version += 1;
        let root = self.accumulator.get_compressed().unwrap_or_default();
        self.blockchain.store_root(self.current_version, root);
    }

    /// Generates a proof for a single element for the current version.
    pub fn generate_proof(&self, element: &[u8]) -> Option<SecureMultisetHash> {
        self.accumulator.generate_proof(element)
    }

    /// Generates proofs for multiple elements in bulk for the current version.
    pub fn generate_proofs_bulk(&self, elements: &[&[u8]]) -> Vec<Option<SecureMultisetHash>> {
        elements
            .iter()
            .map(|e| self.accumulator.generate_proof(e))
            .collect()
    }

    /// Generates a proof for an element for a specific version (regeneration).
    pub fn regenerate_proof_for_version(&self, element: &[u8], version: u64) -> Option<SecureMultisetHash> {
        if let Some(root) = self.blockchain.get_root(version) {
            let accumulator_state = SecureMultisetHash::from_compressed(root);
            accumulator_state.generate_proof(element)
        } else {
            None
        }
    }

    /// Gets the current accumulator root.
    pub fn get_root(&self) -> Vec<u8> {
        self.accumulator.get_compressed().unwrap_or_default()
    }

    /// Creates a client with the element's proof for the current version.
    pub fn create_client<'a>(&self, element: &'a [u8]) -> Option<Client<'a>> {
        if let Some(proof) = self.generate_proof(element) {
            if let Some(root_data) = self.blockchain.get_root(self.current_version) {
                let accumulator_root = SecureMultisetHash::from_compressed(root_data);
                Some(Client::new(
                    element,
                    proof,
                    self.current_version,
                    accumulator_root,
                ))
            } else {
                None
            }
        } else {
            None
        }
    }

    /// Creates a client with the element's proof for a specific version.
    pub fn create_client_for_version<'a>(&self, element: &'a [u8], version: u64) -> Option<Client<'a>> {
        if let Some(proof) = self.regenerate_proof_for_version(element, version) {
            if let Some(root_data) = self.blockchain.get_root(version) {
                let accumulator_root = SecureMultisetHash::from_compressed(root_data);
                Some(Client::new(
                    element,
                    proof,
                    version,
                    accumulator_root,
                ))
            } else {
                None
            }
        } else {
            None
        }
    }
}

/// Represents a client holding a credential and its proof with version information.
pub struct Client<'a> {
    pub element: &'a [u8],
    pub proof: SecureMultisetHash,
    pub version: u64,
    pub accumulator_root: SecureMultisetHash,
}

impl<'a> Client<'a> {
    pub fn new(element: &'a [u8], proof: SecureMultisetHash, version: u64, accumulator_root: SecureMultisetHash) -> Self {
        Client {
            element,
            proof,
            version,
            accumulator_root,
        }
    }

    /// Verifies the proof against the stored accumulator root.
    pub fn verify_proof(&self) -> bool {
        self.accumulator_root.verify_proof(self.element, &self.proof)
    }

    /// Verifies the proof against a given accumulator root.
    pub fn verify_proof_with_root(&self, root: &SecureMultisetHash) -> bool {
        root.verify_proof(self.element, &self.proof)
    }

    /// Gets the version of the client's proof.
    pub fn get_version(&self) -> u64 {
        self.version
    }

    /// Updates the client's proof and version (would be called by issuer).
    pub fn update_proof(&mut self, new_proof: SecureMultisetHash, new_version: u64, new_accumulator_root: SecureMultisetHash) {
        self.proof = new_proof;
        self.version = new_version;
        self.accumulator_root = new_accumulator_root;
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkResult {
    pub name: String,
    pub duration_ms: f64,
    pub operations: Option<usize>,
    pub throughput_ops_per_sec: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub element_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avg_latency_ms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_latency_ms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_latency_ms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blockchain_confirmation_ms: Option<f64>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ExperimentResults {
    pub experiment_name: String,
    pub timestamp: String,
    pub total_duration_ms: f64,
    pub metrics: HashMap<String, BenchmarkResult>,
}

impl ExperimentResults {
    pub fn new(name: &str) -> Self {
        ExperimentResults {
            experiment_name: name.to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            total_duration_ms: 0.0,
            metrics: HashMap::new(),
        }
    }

    pub fn set_total_duration(&mut self, duration: Duration) {
        self.total_duration_ms = duration.as_secs_f64() * 1000.0;
    }

    pub fn get_total_duration(&self) -> Duration {
        Duration::from_millis(self.total_duration_ms as u64)
    }

    pub fn add_labeled_metric(
        &mut self,
        label: &str,
        element_count: usize,
        duration: Duration,
        operations: Option<usize>,
    ) {
        let duration_ms = duration.as_secs_f64() * 1000.0;
        let throughput = operations.map(|ops| ops as f64 / duration.as_secs_f64());

        let key = format!("{}_{}", label, element_count);

        self.metrics.insert(
            key,
            BenchmarkResult {
                name: label.to_string(),
                duration_ms,
                operations,
                throughput_ops_per_sec: throughput,
                element_count: Some(element_count),
                avg_latency_ms: None,
                max_latency_ms: None,
                min_latency_ms: None,
                blockchain_confirmation_ms: None,
            },
        );
    }

    pub fn add_throughput_metric(
        &mut self,
        label: &str,
        element_count: usize,
        throughput_ops_per_sec: f64,
        duration: Duration,
    ) {
        let duration_ms = duration.as_secs_f64() * 1000.0;
        let operations = Some((throughput_ops_per_sec * duration.as_secs_f64()) as usize);

        let key = format!("{}_{}", label, element_count);

        self.metrics.insert(
            key,
            BenchmarkResult {
                name: label.to_string(),
                duration_ms,
                operations,
                throughput_ops_per_sec: Some(throughput_ops_per_sec),
                element_count: Some(element_count),
                avg_latency_ms: None,
                max_latency_ms: None,
                min_latency_ms: None,
                blockchain_confirmation_ms: None,
            },
        );
    }

    pub fn add_batch_latency_metric(
        &mut self,
        label: &str,
        element_count: usize,
        throughput_ops_per_sec: f64,
        duration: Duration,
        avg_latency: Duration,
        max_latency: Duration,
        min_latency: Duration,
        blockchain_confirmation: Duration,
    ) {
        let duration_ms = duration.as_secs_f64() * 1000.0;
        let operations = Some((throughput_ops_per_sec * duration.as_secs_f64()) as usize);

        let key = format!("{}_{}", label, element_count);

        self.metrics.insert(
            key,
            BenchmarkResult {
                name: label.to_string(),
                duration_ms,
                operations,
                throughput_ops_per_sec: Some(throughput_ops_per_sec),
                element_count: Some(element_count),
                avg_latency_ms: Some(avg_latency.as_secs_f64() * 1000.0),
                max_latency_ms: Some(max_latency.as_secs_f64() * 1000.0),
                min_latency_ms: Some(min_latency.as_secs_f64() * 1000.0),
                blockchain_confirmation_ms: Some(blockchain_confirmation.as_secs_f64() * 1000.0),
            },
        );
    }

    pub fn add_latency_metric(
        &mut self,
        label: &str,
        size: usize, // Represents number of operations or a characteristic size
        avg_latency: Duration,
        throughput: f64,
        total_duration: Duration,
    ) {
        let duration_ms = total_duration.as_secs_f64() * 1000.0;
        let operations = Some((throughput * total_duration.as_secs_f64()) as usize);

        // For this metric, the key is just the label, as it's unique per scenario
        let key = label.to_string();

        self.metrics.insert(
            key,
            BenchmarkResult {
                name: label.to_string(),
                duration_ms,
                operations,
                throughput_ops_per_sec: Some(throughput),
                element_count: Some(size),
                avg_latency_ms: Some(avg_latency.as_secs_f64() * 1000.0),
                max_latency_ms: None, // Not measuring max/min for this simple test
                min_latency_ms: None,
                blockchain_confirmation_ms: None,
            },
        );
    }

    pub fn add_lazy_verification_metric(
        &mut self,
        label: &str,
        element_count: usize,
        throughput_ops_per_sec: f64,
        duration: Duration,
        avg_latency: Duration,
        max_latency: Duration,
    ) {
        let duration_ms = duration.as_secs_f64() * 1000.0;
        let operations = Some((throughput_ops_per_sec * duration.as_secs_f64()) as usize);

        let key = format!("{}_{}", label, element_count);

        self.metrics.insert(
            key,
            BenchmarkResult {
                name: label.to_string(),
                duration_ms,
                operations,
                throughput_ops_per_sec: Some(throughput_ops_per_sec),
                element_count: Some(element_count),
                avg_latency_ms: Some(avg_latency.as_secs_f64() * 1000.0),
                max_latency_ms: Some(max_latency.as_secs_f64() * 1000.0),
                min_latency_ms: None,
                blockchain_confirmation_ms: None,
            },
        );
    }

    pub fn save_to_file(&self, filename: &str) -> Result<(), Box<dyn std::error::Error>> {
        let json = serde_json::to_string_pretty(self)?;
        fs::write(filename, json)?;
        Ok(())
    }

    pub fn merge_results(&mut self, other: ExperimentResults) {
        // Merge metrics from another ExperimentResults
        for (key, result) in other.metrics {
            self.metrics.insert(key, result);
        }
    }
}

/// Utility function to generate test elements
pub fn generate_test_elements(count: usize) -> Vec<Vec<u8>> {
    (0..count)
        .map(|i| format!("element{}", i).into_bytes())
        .collect()
}

/// Utility function to convert Vec<Vec<u8>> to Vec<&[u8]>
pub fn to_element_slices(elements: &[Vec<u8>]) -> Vec<&[u8]> {
    elements.iter().map(|e| e.as_slice()).collect()
}

/// Utility function to print benchmark summary
pub fn print_summary(results: &ExperimentResults) {
    println!("\n=== Benchmark Summary ===");
    println!("Total experiment time: {:.2} ms", results.total_duration_ms);

    // Group results by label
    let mut grouped_results = std::collections::HashMap::new();
    for (key, result) in &results.metrics {
        if result.name.contains('.') {
            grouped_results
                .entry(result.name.clone())
                .or_insert_with(Vec::new)
                .push((key, result));
        }
    }

    for (label, results) in grouped_results {
        println!("\n{}:", label);
        for (key, result) in results {
            if let Some(throughput) = result.throughput_ops_per_sec {
                println!(
                    "  {}: {:.2} ms, {:.2} ops/sec",
                    key, result.duration_ms, throughput
                );
            } else {
                println!("  {}: {:.2} ms", key, result.duration_ms);
            }
        }
    }
}

#[cfg(test)]
mod version_tests {
    use super::*;

    #[test]
    fn test_blockchain_version_management() {
        let mut blockchain = Blockchain::new();
        assert_eq!(blockchain.get_current_version(), 0);

        let root1 = vec![1, 2, 3, 4];
        let root2 = vec![5, 6, 7, 8];

        blockchain.store_root(1, root1.clone());
        assert_eq!(blockchain.get_current_version(), 1);
        assert_eq!(blockchain.get_root(1), Some(&root1));

        blockchain.store_root(3, root2.clone());
        assert_eq!(blockchain.get_current_version(), 3);
        assert_eq!(blockchain.get_root(3), Some(&root2));
        assert_eq!(blockchain.get_root(1), Some(&root1));
    }

    #[test]
    fn test_issuer_version_increments() {
        let mut issuer = Issuer::new();
        assert_eq!(issuer.get_current_version(), 1); // starts at 1

        let elements: Vec<&[u8]> = vec![b"element1", b"element2"];
        issuer.add_elements(&elements);
        assert_eq!(issuer.get_current_version(), 2);

        issuer.remove_elements(&[b"element1"]);
        assert_eq!(issuer.get_current_version(), 3);
    }

    #[test]
    fn test_proof_generation() {
        let mut issuer = Issuer::new();
        let element = b"test_element";

        // Add element
        issuer.add_elements(&[element]);
        let _version_after_add = issuer.get_current_version();

        // Generate proof for the element
        let proof = issuer.generate_proof(element);
        assert!(proof.is_some());

        // Add another element to increment version
        issuer.add_elements(&[b"another_element"]);

        // Can still generate proof for original element
        let original_proof = issuer.generate_proof(element);
        assert!(original_proof.is_some());

        // Generate proof for new element
        let new_element = b"another_element";
        let new_proof = issuer.generate_proof(new_element);
        assert!(new_proof.is_some());
    }

    #[test]
    fn test_client_creation_and_verification() {
        let mut issuer = Issuer::new();
        let element = b"client_test_element";

        // Add element
        issuer.add_elements(&[element]);

        // Create client for current version
        let client = issuer.create_client(element).unwrap();
        assert_eq!(client.get_version(), issuer.get_current_version());
        assert!(client.verify_proof());

        // Test verification with wrong root should fail
        let wrong_root = SecureMultisetHash::new();
        assert!(!client.verify_proof_with_root(&wrong_root));
    }

    #[test]
    fn test_version_isolation() {
        let mut issuer = Issuer::new();
        let element = b"isolation_test";

        // Add element to create version 2
        issuer.add_elements(&[element]);
        let _version2 = issuer.get_current_version();

        // Create client for version 2
        let client_v2 = issuer.create_client(element).unwrap();
        assert_eq!(client_v2.get_version(), 2);
        assert!(client_v2.verify_proof());

        // Add more elements to create version 3
        issuer.add_elements(&[b"another_element"]);
        let _version3 = issuer.get_current_version();

        // Original client should still verify with version 2
        assert!(client_v2.verify_proof());

        // Try to create client for version 2 - should still work
        let client_v2_again = issuer.create_client_for_version(element, 2).unwrap();
        assert_eq!(client_v2_again.get_version(), 2);
        assert!(client_v2_again.verify_proof());

        // Verify both clients have the same accumulator root for version 2
        let root_v2_data = issuer.blockchain.get_root(2).unwrap();
        let root_v2 = SecureMultisetHash::from_compressed(root_v2_data);
        assert!(client_v2.verify_proof_with_root(&root_v2));
        assert!(client_v2_again.verify_proof_with_root(&root_v2));
    }

    #[test]
    fn test_regenerate_proof_for_version() {
        let mut issuer = Issuer::new();
        let element = b"regeneration_test";

        // Add element to create version 2
        issuer.add_elements(&[element]);
        let _version2 = issuer.get_current_version();

        // Add more elements to create version 3
        issuer.add_elements(&[b"another_element"]);

        // Regenerate proof for version 2
        let regenerated_proof = issuer.regenerate_proof_for_version(element, 2);
        assert!(regenerated_proof.is_some());

        // Create client for version 2 and verify
        let client = issuer.create_client_for_version(element, 2).unwrap();
        assert!(client.verify_proof());

        // Verify the regenerated proof works with version 2 root
        let root_v2_data = issuer.blockchain.get_root(2).unwrap();
        let root_v2 = SecureMultisetHash::from_compressed(root_v2_data);
        assert!(root_v2.verify_proof(element, &regenerated_proof.unwrap()));
    }

    #[test]
    fn test_multiple_proof_generation() {
        let mut issuer = Issuer::new();
        let element = b"hash_test";

        // Add element
        issuer.add_elements(&[element]);
        let version = issuer.get_current_version();

        // Generate proof for element
        let proof1 = issuer.generate_proof(element);
        assert!(proof1.is_some());

        // Generate proof again for same element
        let proof2 = issuer.generate_proof(element);
        assert!(proof2.is_some());

        // Both proofs should work with the same root
        let root_data = issuer.blockchain.get_root(version).unwrap();
        let root = SecureMultisetHash::from_compressed(root_data);
        assert!(root.verify_proof(element, &proof1.unwrap()));
        assert!(root.verify_proof(element, &proof2.unwrap()));

        // Generate proof for different element
        let different_element = b"different_element";
        let different_proof = issuer.generate_proof(different_element);
        assert!(different_proof.is_some());
    }
}
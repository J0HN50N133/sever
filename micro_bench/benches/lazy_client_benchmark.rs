mod batch_common;
pub mod common;

use crate::common::{
    generate_test_elements, print_summary, to_element_slices, ExperimentResults, Issuer,
};
use batch_common::BatchProcessor;
use log::{debug, error, info};
use minchash::{MultisetHash, SecureMultisetHash};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

const TOTAL_CREDENTIALS: usize = 10_000;
const BLOCKCHAIN_TPS: u64 = 500;
const BATCH_SIZES: &[usize] = &[1, 10, 50, 100, 500, 1000];
const REVOCATION_PERCENTAGES: &[f64] = &[0.10, 0.25, 0.50];

/// Simple Bloom Filter implementation for revocation list
#[derive(Debug, Clone)]
struct BloomFilter {
    bits: Vec<bool>,
    num_hashes: u32,
}

impl BloomFilter {
    fn new(expected_elements: usize, false_positive_rate: f64) -> Self {
        let m = ((expected_elements as f64 * (-false_positive_rate.ln())) / (2.0f64.ln()).powi(2))
            .ceil() as usize;
        let k = ((m as f64 * 2.0f64.ln()) / expected_elements as f64).ceil() as u32;

        BloomFilter {
            bits: vec![false; m],
            num_hashes: k,
        }
    }

    fn add(&mut self, element: &[u8]) {
        for i in 0..self.num_hashes {
            let hash = self.hash_with_seed(element, i);
            let index = (hash % self.bits.len() as u64) as usize;
            self.bits[index] = true;
        }
    }

    fn contains(&self, element: &[u8]) -> bool {
        for i in 0..self.num_hashes {
            let hash = self.hash_with_seed(element, i);
            let index = (hash % self.bits.len() as u64) as usize;
            if !self.bits[index] {
                return false; // Definitely not in the set
            }
        }
        true // Possibly in the set (could be false positive)
    }

    fn hash_with_seed(&self, element: &[u8], seed: u32) -> u64 {
        // Simple hash function for demonstration
        let mut hash = seed as u64;
        for &byte in element {
            hash = hash.wrapping_mul(31).wrapping_add(byte as u64);
        }
        hash
    }
}

/// Represents the verifier with local caching and revocation checking using bloom filter
#[derive(Debug, Clone)]
struct LazyVerifier {
    /// Local cache of historical accumulator roots (version -> root)
    cache: HashMap<u64, SecureMultisetHash>,
    /// Maximum number of versions to cache
    cache_size: usize,
    /// Bloom filter for revocation checking (no false negatives)
    revocation_bloom: BloomFilter,
    /// Statistics for bloom filter performance
    bloom_check_count: u64,
    bloom_positive_count: u64,
}

impl LazyVerifier {
    /// Creates a new LazyVerifier with specified cache size and expected revoked elements
    fn new(cache_size: usize, expected_revoked_elements: usize) -> Self {
        LazyVerifier {
            cache: HashMap::new(),
            cache_size,
            revocation_bloom: BloomFilter::new(expected_revoked_elements.max(1), 0.01), // 1% false positive rate
            bloom_check_count: 0,
            bloom_positive_count: 0,
        }
    }

    /// Adds a historical root to the cache (LRU eviction when full)
    fn add_to_cache(&mut self, version: u64, root: SecureMultisetHash) {
        if self.cache.len() >= self.cache_size {
            let oldest_key = *self.cache.keys().min().unwrap_or(&0);
            self.cache.remove(&oldest_key);
        }
        self.cache.insert(version, root);
    }

    /// Checks if an element is revoked using bloom filter
    fn is_revoked(&mut self, element: &[u8]) -> bool {
        let result = self.revocation_bloom.contains(element);
        if result {
            self.bloom_positive_count += 1;
        }
        self.bloom_check_count += 1;
        result
    }

    /// Adds a revocation to the bloom filter
    fn add_revocation(&mut self, element: &[u8]) {
        self.revocation_bloom.add(element);
    }

    /// Optimized verification logic for lazy witness update
    fn verify_lazy(
        &mut self,
        element: &[u8],
        proof: &SecureMultisetHash,
        client_version: u64,
    ) -> VerificationResult {
        // 1. Revocation Check using Bloom Filter
        let bloom_check_start = Instant::now();
        if self.is_revoked(element) {
            return VerificationResult::Rejected {
                reason: "Element revoked (bloom filter check)".to_string(),
                verification_latency: bloom_check_start.elapsed(),
                cache_hit: false,
            };
        }

        // 2. Local Cache Lookup
        if let Some(cached_root) = self.cache.get(&client_version) {
            // 3. Safe Verification using cached root
            let crypto_start = Instant::now();
            let is_valid = cached_root.verify_proof(element, proof);
            let crypto_latency = crypto_start.elapsed();

            if is_valid {
                return VerificationResult::Accepted {
                    verification_latency: crypto_latency,
                    cache_hit: true,
                };
            } else {
                return VerificationResult::Rejected {
                    reason: "Invalid proof".to_string(),
                    verification_latency: crypto_latency,
                    cache_hit: true,
                };
            }
        }

        // Fallback: No cache entry for this version
        VerificationResult::FallbackRequired {
            reason: format!("Cache miss for version {}", client_version),
        }
    }
}

/// Result of lazy verification
#[derive(Debug, Clone)]
enum VerificationResult {
    Accepted {
        verification_latency: Duration,
        cache_hit: bool,
    },
    Rejected {
        reason: String,
        verification_latency: Duration,
        cache_hit: bool,
    },
    FallbackRequired {
        reason: String,
    },
}

/// Represents a client with a credential (element and proof) at a specific version.
#[derive(Clone)]
struct LazyClient {
    element: Vec<u8>,
    proof: SecureMultisetHash,
    version: u64,
}

impl LazyClient {
    fn new(element: &[u8], proof: SecureMultisetHash, version: u64) -> Self {
        LazyClient {
            element: element.to_vec(),
            proof,
            version,
        }
    }
}

/// Scenario S1: Issuance Testing - Add credentials in batches
fn run_s1_issuance_tests(results: &mut ExperimentResults) {
    info!("Starting S1: Issuance Testing");
    debug!("Testing batch sizes: {:?}", BATCH_SIZES);

    for &batch_size in BATCH_SIZES {
        info!("Testing batch size: {}", batch_size);

        let elements = generate_test_elements(TOTAL_CREDENTIALS);
        let element_slices = to_element_slices(&elements);

        let mut issuer = Issuer::new();
        let mut batch_processor = BatchProcessor::new(BLOCKCHAIN_TPS);

        let test_start = Instant::now();
        let mut batch_metrics = Vec::new();
        for chunk in element_slices.chunks(batch_size) {
            let metrics = batch_processor.process_add_batch(&mut issuer, chunk);
            batch_metrics.push(metrics);
        }
        let total_test_time = test_start.elapsed();

        let total_operations: usize = batch_metrics.iter().map(|m| m.batch_size).sum();
        let overall_throughput = total_operations as f64 / total_test_time.as_secs_f64();

        results.add_throughput_metric(
            "S1-Issuance",
            batch_size,
            overall_throughput,
            total_test_time,
        );
    }
}

/// Scenario S2: Revocation Testing - Revoke credentials in batches
fn run_s2_revocation_tests(results: &mut ExperimentResults) {
    info!("Starting S2: Revocation Testing");

    for &revocation_percentage in REVOCATION_PERCENTAGES {
        let revocation_count = (TOTAL_CREDENTIALS as f64 * revocation_percentage) as usize;
        let label_suffix = format!("{}%", (revocation_percentage * 100.0) as usize);

        info!(
            "Testing revocation: {} ({})",
            label_suffix, revocation_count
        );

        for &batch_size in BATCH_SIZES {
            let elements = generate_test_elements(TOTAL_CREDENTIALS);
            let element_slices = to_element_slices(&elements);
            let mut issuer = Issuer::new();
            issuer.add_elements(&element_slices);

            let elements_to_revoke = &element_slices[0..revocation_count];
            let mut batch_processor = BatchProcessor::new(BLOCKCHAIN_TPS);

            let test_start = Instant::now();
            let mut batch_metrics = Vec::new();
            for chunk in elements_to_revoke.chunks(batch_size) {
                let metrics = batch_processor.process_remove_batch(&mut issuer, chunk);
                batch_metrics.push(metrics);
            }
            let total_test_time = test_start.elapsed();

            let total_operations: usize = batch_metrics.iter().map(|m| m.batch_size).sum();
            let overall_throughput = total_operations as f64 / total_test_time.as_secs_f64();

            let revoke_scenario_name = format!("S2-Revoke{}", label_suffix);
            results.add_throughput_metric(
                &revoke_scenario_name,
                batch_size,
                overall_throughput,
                total_test_time,
            );
        }
    }
}

/// Scenario S3: Concurrent Lazy Verification Testing
fn run_s3_lazy_concurrent_verification_tests(results: &mut ExperimentResults) {
    info!("Starting S3: Concurrent Lazy Verification Testing");

    const NUM_CLIENTS: usize = 4;
    const VERIFICATION_INTERVAL_MS: u64 = 10;
    const VERIFIER_CACHE_SIZE: usize = 100;

    for &revocation_percentage in REVOCATION_PERCENTAGES {
        let revocation_count = (TOTAL_CREDENTIALS as f64 * revocation_percentage) as usize;
        let label_suffix = format!("{}%", (revocation_percentage * 100.0) as usize);

        info!(
            "Testing concurrent lazy verification during {} revocation",
            label_suffix
        );

        for &batch_size in BATCH_SIZES {
            debug!(
                "  Testing with batch size {} and revocation {}",
                batch_size, label_suffix
            );

            let elements = generate_test_elements(TOTAL_CREDENTIALS);
            let element_slices = to_element_slices(&elements);

            let mut issuer = Issuer::new();
            issuer.add_elements(&element_slices);
            let initial_version = issuer.get_current_version();
            let initial_root_data = issuer.get_blockchain().get_root(initial_version).unwrap();
            let initial_root = SecureMultisetHash::from_compressed(initial_root_data);

            let elements_to_revoke: Vec<Vec<u8>> = element_slices[0..revocation_count]
                .iter()
                .map(|s| s.to_vec())
                .collect();
            let verification_elements: Vec<Vec<u8>> = element_slices[revocation_count..]
                .iter()
                .map(|s| s.to_vec())
                .collect();

            let clients: Vec<LazyClient> = verification_elements
                .iter()
                .map(|elem| {
                    let proof = issuer.generate_proof(elem).unwrap();
                    LazyClient::new(elem, proof, initial_version)
                })
                .collect();

            let issuer_arc = Arc::new(Mutex::new(issuer));
            let clients_arc = Arc::new(clients);

            let mut client_handles = Vec::new();
            for _ in 0..NUM_CLIENTS {
                let _issuer_clone = Arc::clone(&issuer_arc);
                let clients_clone = Arc::clone(&clients_arc);
                let elements_to_revoke_clone = elements_to_revoke.clone();
                let initial_root_clone = initial_root.clone();

                let handle = thread::spawn(move || {
                    let mut verifier = LazyVerifier::new(VERIFIER_CACHE_SIZE, revocation_count);
                    verifier.add_to_cache(initial_version, initial_root_clone);
                    for elem in &elements_to_revoke_clone {
                        verifier.add_revocation(elem);
                    }

                    let mut verification_latencies = Vec::new();
                    let mut cache_hits = 0;
                    let mut fallbacks = 0;
                    let mut verifications_performed = 0;

                    loop {
                        if clients_clone.is_empty() {
                            break;
                        }
                        let client = &clients_clone[verifications_performed % clients_clone.len()];

                        let result =
                            verifier.verify_lazy(&client.element, &client.proof, client.version);

                        match result {
                            VerificationResult::Accepted {
                                verification_latency,
                                cache_hit,
                            } => {
                                verification_latencies.push(verification_latency);
                                if cache_hit {
                                    cache_hits += 1;
                                }
                            }
                            VerificationResult::Rejected { .. } => { /* This shouldn't happen for valid clients */
                            }
                            VerificationResult::FallbackRequired { .. } => {
                                fallbacks += 1;
                            }
                        }

                        verifications_performed += 1;
                        if verifications_performed >= 1000 {
                            break;
                        } // Limit total verifications per thread
                        thread::sleep(Duration::from_millis(VERIFICATION_INTERVAL_MS));
                    }

                    (
                        verification_latencies,
                        cache_hits,
                        fallbacks,
                        verifications_performed,
                    )
                });
                client_handles.push(handle);
            }

            let revocation_start = Instant::now();
            let mut batch_processor = BatchProcessor::new(BLOCKCHAIN_TPS);
            for chunk in elements_to_revoke.chunks(batch_size) {
                let mut issuer_guard = issuer_arc.lock().unwrap();
                let chunk_refs: Vec<&[u8]> = chunk.iter().map(|v| v.as_slice()).collect();
                batch_processor.process_remove_batch(&mut issuer_guard, &chunk_refs);
            }
            let revocation_duration = revocation_start.elapsed();

            let mut all_latencies = Vec::new();
            // let mut _total_cache_hits = 0;
            // let mut _total_fallbacks = 0;
            let mut total_verifications = 0;

            for handle in client_handles {
                let (latencies, _, _, num_verifications) = handle.join().unwrap();
                all_latencies.extend(latencies);
                // total_cache_hits += cache_hits;
                // total_fallbacks += fallbacks;
                total_verifications += num_verifications;
            }

            let total_verification_time: Duration = all_latencies.iter().sum();
            let avg_latency = if !all_latencies.is_empty() {
                total_verification_time / all_latencies.len() as u32
            } else {
                Duration::ZERO
            };
            let max_latency = all_latencies
                .iter()
                .max()
                .cloned()
                .unwrap_or(Duration::ZERO);
            let overall_throughput = total_verifications as f64 / revocation_duration.as_secs_f64();

            let concurrent_scenario_name = format!("S3-Concurrent-Lazy-{}", label_suffix);
            results.add_batch_latency_metric(
                &concurrent_scenario_name,
                batch_size,
                overall_throughput,
                revocation_duration,
                avg_latency,
                max_latency,
                Duration::ZERO,
                Duration::ZERO,
            );
        }
    }
}

fn main() {
    logforth::starter_log::stdout().apply();
    info!("Starting Lazy Client Performance Benchmarks...");
    info!("Configuration:");
    info!("  Total credentials: {}", TOTAL_CREDENTIALS);
    info!("  Blockchain TPS limit: {}", BLOCKCHAIN_TPS);
    info!("  Batch sizes to test: {:?}", BATCH_SIZES);
    info!("  Revocation percentages: {:?}", REVOCATION_PERCENTAGES);

    let experiment_start = Instant::now();
    let mut results = ExperimentResults::new("minchash_secure_lazy_batch_performance");

    run_s1_issuance_tests(&mut results);
    run_s2_revocation_tests(&mut results);

    let mut verification_results =
        ExperimentResults::new("minchash_secure_lazy_concurrent_verification");
    run_s3_lazy_concurrent_verification_tests(&mut verification_results);

    let total_duration = experiment_start.elapsed();
    results.set_total_duration(total_duration);
    verification_results.set_total_duration(total_duration);

    let batch_output_file = "lazy_batch_benchmark_results.json";
    if let Err(e) = results.save_to_file(batch_output_file) {
        error!("Failed to save batch results: {}", e);
    } else {
        info!("Batch results saved to {}", batch_output_file);
    }

    let verification_output_file = "lazy_verification_results.json";
    if let Err(e) = verification_results.save_to_file(verification_output_file) {
        error!("Failed to save verification results: {}", e);
    } else {
        info!(
            "Concurrent verification results saved to {}",
            verification_output_file
        );
    }

    info!(
        "Total experiment time: {:.2}s",
        total_duration.as_secs_f64()
    );
    print_summary(&results);
    print_summary(&verification_results);
}

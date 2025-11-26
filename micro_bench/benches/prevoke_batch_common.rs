// This file is an adaptation of batch_common.rs for the Prevoke scheme.

use crate::common::ExperimentResults;
use log::{debug, info};
use rand::TryRngCore as _;
use rand::{rngs::OsRng, RngCore};
use rs_merkle::{algorithms::Sha256, MerkleProof, MerkleTree};
use sha2::{Digest, Sha256 as Sha256Hasher};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

// --- PrevokeIssuer struct, copied and extended from prevoke_issuer_benchmark.rs ---
#[derive(Clone)]
pub struct PrevokeIssuer {
    pub tree: Option<MerkleTree<Sha256>>,
    pub leaves: Vec<[u8; 32]>,
    pub bloom_filter: Vec<u8>,
    m: usize,
    k: usize,
}

impl Default for PrevokeIssuer {
    fn default() -> Self {
        Self::new()
    }
}

impl PrevokeIssuer {
    pub fn new() -> Self {
        let m = 20_000_000; // 20M bits for Bloom Filter
        let k = 7;
        PrevokeIssuer {
            tree: None,
            leaves: Vec::new(),
            bloom_filter: vec![0; (m + 7) / 8],
            m,
            k,
        }
    }

    pub fn add_elements(&mut self, elements: &[Vec<u8>]) -> Vec<usize> {
        if elements.is_empty() {
            return vec![];
        }
        let mut new_leaves: Vec<_> = elements
            .iter()
            .map(|element| {
                let mut h = Sha256Hasher::new();
                h.update(element);
                let leaf_hash: [u8; 32] = h.finalize().into();
                leaf_hash
            })
            .collect();

        let start_index = self.leaves.len();
        let new_indices = (start_index..start_index + new_leaves.len()).collect();

        self.leaves.extend_from_slice(&new_leaves);

        if let Some(tree) = &mut self.tree {
            tree.append(&mut new_leaves).commit();
        } else {
            self.tree = Some(MerkleTree::<Sha256>::from_leaves(&self.leaves));
        }
        new_indices
    }

    pub fn revoke_elements(&mut self, indices: &[usize]) {
        if indices.is_empty() {
            return;
        }
        for &index in indices {
            if index < self.leaves.len() {
                let mut random_hash = [0u8; 32];
                OsRng.try_fill_bytes(&mut random_hash).unwrap();
                self.leaves[index] = random_hash;
            }
        }

        self.tree = Some(MerkleTree::<Sha256>::from_leaves(&self.leaves));
    }

    #[allow(dead_code)]
    fn get_bloom_indexes(&self, item: &[u8]) -> Vec<usize> {
        let mut indexes = Vec::with_capacity(self.k);
        let mut h = Sha256Hasher::new();
        h.update(item);
        let hash1_full = h.finalize();
        let h1 = u64::from_be_bytes(hash1_full[0..8].try_into().unwrap());

        let mut h = Sha256Hasher::new();
        h.update(&hash1_full);
        let hash2_full = h.finalize();
        let h2 = u64::from_be_bytes(hash2_full[0..8].try_into().unwrap());

        for i in 0..self.k {
            let idx = (h1.wrapping_add((i as u64).wrapping_mul(h2))) as usize % self.m;
            indexes.push(idx);
        }
        indexes
    }

    #[allow(dead_code)]
    pub fn check_bloom_filter(&self, item: &[u8]) -> bool {
        let indexes = self.get_bloom_indexes(item);
        for idx in indexes {
            let byte_idx = idx / 8;
            let bit_idx = idx % 8;
            if (self.bloom_filter[byte_idx] >> bit_idx) & 1 == 0 {
                return false;
            }
        }
        true
    }

    pub fn generate_proof(&self, index: usize) -> Option<MerkleProof<Sha256>> {
        self.tree.as_ref().map(|t| t.proof(&[index]))
    }

    pub fn root(&self) -> Option<[u8; 32]> {
        self.tree.as_ref().and_then(|t| t.root())
    }
}

// --- Copied from batch_common.rs ---

#[derive(Debug, Clone)]
pub struct BatchMetrics {
    pub batch_size: usize,
    pub total_duration: Duration,
    pub average_latency: Duration,
    pub max_latency: Duration,
    pub min_latency: Duration,
    pub throughput: f64,
    pub blockchain_confirmation_time: Duration,
}

impl BatchMetrics {
    pub fn new(
        batch_size: usize,
        total_duration: Duration,
        latencies: &[Duration],
        blockchain_time: Duration,
    ) -> Self {
        let avg_latency = if !latencies.is_empty() {
            latencies.iter().sum::<Duration>() / latencies.len() as u32
        } else {
            Duration::ZERO
        };
        let max_latency = *latencies.iter().max().unwrap_or(&Duration::ZERO);
        let min_latency = *latencies.iter().min().unwrap_or(&Duration::ZERO);
        let throughput = if total_duration.as_secs_f64() > 0.0 {
            batch_size as f64 / total_duration.as_secs_f64()
        } else {
            0.0
        };

        Self {
            batch_size,
            total_duration,
            average_latency: avg_latency,
            max_latency,
            min_latency,
            throughput,
            blockchain_confirmation_time: blockchain_time,
        }
    }
}

pub struct TpsLimiter {
    max_tps: u64,
    last_tx_time: Arc<Mutex<Instant>>,
    tx_interval: Duration,
}

impl TpsLimiter {
    pub fn new(max_tps: u64) -> Self {
        let tx_interval = Duration::from_secs_f64(1.0 / max_tps as f64);
        Self {
            max_tps,
            last_tx_time: Arc::new(Mutex::new(Instant::now())),
            tx_interval,
        }
    }

    pub fn wait_for_next_transaction(&self) {
        let mut last_time = self.last_tx_time.lock().unwrap();
        let elapsed = last_time.elapsed();

        if elapsed < self.tx_interval {
            thread::sleep(self.tx_interval - elapsed);
        }

        *last_time = Instant::now();
    }

    pub fn get_blockchain_confirmation_time(&self, num_transactions: usize) -> Duration {
        Duration::from_secs_f64(num_transactions as f64 / self.max_tps as f64)
    }
}

// --- BatchProcessor modified for Prevoke ---
pub struct BatchProcessor {
    tps_limiter: TpsLimiter,
    operation_latencies: Vec<Duration>,
}

impl BatchProcessor {
    pub fn new(tps_limit: u64) -> Self {
        Self {
            tps_limiter: TpsLimiter::new(tps_limit),
            operation_latencies: Vec::new(),
        }
    }

    pub fn process_add_batch(
        &mut self,
        issuer: &mut PrevokeIssuer,
        elements: &[Vec<u8>],
    ) -> BatchMetrics {
        let start_time = Instant::now();

        let op_start = Instant::now();
        issuer.add_elements(elements);
        let op_duration = op_start.elapsed();

        let latencies = if !elements.is_empty() {
            vec![op_duration / elements.len() as u32; elements.len()]
        } else {
            vec![]
        };

        let blockchain_start = Instant::now();
        self.tps_limiter.wait_for_next_transaction();
        let blockchain_duration = blockchain_start.elapsed();

        let total_duration = start_time.elapsed();
        let blockchain_confirmation_time = self.tps_limiter.get_blockchain_confirmation_time(1);

        let metrics = BatchMetrics::new(
            elements.len(),
            total_duration,
            &latencies,
            blockchain_confirmation_time,
        );
        self.operation_latencies.extend(latencies);

        debug!(
            "Prevoke Add batch completed: {:.2}s total ({:.2}s local + {:.2}s blockchain), {:.2} ops/s",
            total_duration.as_secs_f64(),
            (total_duration - blockchain_duration).as_secs_f64(),
            blockchain_duration.as_secs_f64(),
            metrics.throughput
        );

        metrics
    }

    pub fn process_remove_batch(
        &mut self,
        issuer: &mut PrevokeIssuer,
        indices: &[usize],
    ) -> BatchMetrics {
        let start_time = Instant::now();

        let op_start = Instant::now();
        issuer.revoke_elements(indices);
        let op_duration = op_start.elapsed();

        let latencies = if !indices.is_empty() {
            vec![op_duration / indices.len() as u32; indices.len()]
        } else {
            vec![]
        };

        let blockchain_start = Instant::now();
        self.tps_limiter.wait_for_next_transaction();
        let blockchain_duration = blockchain_start.elapsed();

        let total_duration = start_time.elapsed();
        let blockchain_confirmation_time = self.tps_limiter.get_blockchain_confirmation_time(1);

        let metrics = BatchMetrics::new(
            indices.len(),
            total_duration,
            &latencies,
            blockchain_confirmation_time,
        );
        self.operation_latencies.extend(latencies);

        debug!(
            "Prevoke Remove batch completed: {:.2}s total ({:.2}s local + {:.2}s blockchain), {:.2} ops/s",
            total_duration.as_secs_f64(),
            (total_duration - blockchain_duration).as_secs_f64(),
            blockchain_duration.as_secs_f64(),
            metrics.throughput
        );

        metrics
    }

    #[allow(dead_code)]
    pub fn get_operation_stats(&self) -> (Duration, Duration, f64) {
        if self.operation_latencies.is_empty() {
            return (Duration::ZERO, Duration::ZERO, 0.0);
        }

        let total_ops = self.operation_latencies.len();
        let total_time: Duration = self.operation_latencies.iter().sum();
        let avg_latency = total_time / total_ops as u32;
        let max_latency = *self
            .operation_latencies
            .iter()
            .max()
            .unwrap_or(&Duration::ZERO);
        let avg_throughput = total_ops as f64
            / self
                .operation_latencies
                .iter()
                .map(|d| d.as_secs_f64())
                .sum::<f64>();

        (avg_latency, max_latency, avg_throughput)
    }
}

// --- VerificationClient modified for Prevoke ---
pub struct VerificationClient {
    client_id: usize,
    verification_latencies: Vec<Duration>,
}

impl VerificationClient {
    pub fn new(client_id: usize) -> Self {
        Self {
            client_id,
            verification_latencies: Vec::new(),
        }
    }

    pub fn verify_concurrent(
        &mut self,
        issuer: &PrevokeIssuer,
        items: &[(Vec<u8>, usize)],
    ) -> (Duration, Duration, f64) {
        let start_time = Instant::now();
        let mut latencies = Vec::new();

        debug!(
            "Client {} starting prevoke verification of {} elements",
            self.client_id,
            items.len()
        );

        let root = issuer.root().expect("Tree has no root");
        let num_leaves = issuer.leaves.len();

        for (element, index) in items {
            let verify_start = Instant::now();

            if let Some(proof) = issuer.generate_proof(*index) {
                let mut h = Sha256Hasher::new();
                h.update(element);
                let leaf_hash: [u8; 32] = h.finalize().into();

                let verified = proof.verify(root, &[*index], &[leaf_hash], num_leaves);
                assert!(
                    verified,
                    "Prevoke verification failed for client {} on index {}",
                    self.client_id, index
                );
            } else {
                panic!("Failed to generate proof for index {}", index);
            }

            let verify_duration = verify_start.elapsed();
            latencies.push(verify_duration);
        }

        let total_time = start_time.elapsed();
        let (avg_latency, max_latency, throughput) = if !latencies.is_empty() {
            let avg = latencies.iter().sum::<Duration>() / latencies.len() as u32;
            let max = *latencies.iter().max().unwrap_or(&Duration::ZERO);
            let tp = if total_time.as_secs_f64() > 0.0 {
                items.len() as f64 / total_time.as_secs_f64()
            } else {
                0.0
            };
            (avg, max, tp)
        } else {
            (Duration::ZERO, Duration::ZERO, 0.0)
        };

        self.verification_latencies.extend(latencies);

        info!(
            "Client {} prevoke verification: avg {:.2}ms, max {:.2}ms, {:.2} verifications/s",
            self.client_id,
            avg_latency.as_secs_f64() * 1000.0,
            max_latency.as_secs_f64() * 1000.0,
            throughput
        );

        (avg_latency, max_latency, throughput)
    }

    pub fn get_verification_stats(&self) -> (Duration, Duration, f64) {
        if self.verification_latencies.is_empty() {
            return (Duration::ZERO, Duration::ZERO, 0.0);
        }

        let total_verifications = self.verification_latencies.len();
        let total_time: Duration = self.verification_latencies.iter().sum();
        let avg_latency = total_time / total_verifications as u32;
        let max_latency = *self
            .verification_latencies
            .iter()
            .max()
            .unwrap_or(&Duration::ZERO);
        let throughput = if total_time.as_secs_f64() > 0.0 {
            total_verifications as f64 / total_time.as_secs_f64()
        } else {
            0.0
        };

        (avg_latency, max_latency, throughput)
    }
}

impl ExperimentResults {
    pub fn add_batch_metrics(&mut self, label: &str, _batch_index: usize, metrics: &BatchMetrics) {
        self.add_labeled_metric(
            label,
            metrics.batch_size,
            metrics.total_duration,
            Some(metrics.batch_size),
        );
        self.add_labeled_metric(
            &format!("{}-avg_latency", label),
            metrics.batch_size,
            metrics.average_latency,
            Some(metrics.batch_size),
        );
        self.add_labeled_metric(
            &format!("{}-max_latency", label),
            metrics.batch_size,
            metrics.max_latency,
            Some(metrics.batch_size),
        );
        if metrics.throughput > 0.0 {
            self.add_labeled_metric(
                &format!("{}-throughput", label),
                metrics.batch_size,
                Duration::from_secs_f64(1.0 / metrics.throughput),
                Some(metrics.batch_size),
            );
        }
        self.add_labeled_metric(
            &format!("{}-blockchain_time", label),
            metrics.batch_size,
            metrics.blockchain_confirmation_time,
            Some(metrics.batch_size),
        );
    }

    pub fn add_verification_metrics(
        &mut self,
        label: &str,
        batch_size: usize,
        avg_latency: Duration,
        max_latency: Duration,
        throughput: f64,
    ) {
        self.add_labeled_metric(
            &format!("{}-verification_avg", label),
            batch_size,
            avg_latency,
            Some(1),
        );
        self.add_labeled_metric(
            &format!("{}-verification_max", label),
            batch_size,
            max_latency,
            Some(1),
        );
        if throughput > 0.0 {
            self.add_labeled_metric(
                &format!("{}-verification_throughput", label),
                batch_size,
                Duration::from_secs_f64(1.0 / throughput),
                Some(1),
            );
        }
    }
}

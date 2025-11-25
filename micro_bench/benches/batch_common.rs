use crate::common::{ExperimentResults, Issuer};
use log::{debug, info};
use minchash::MultisetHash;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

/// Represents a single operation in a batch
#[derive(Debug, Clone)]
pub struct BatchOperation {
    pub id: usize,
    pub operation_type: OperationType,
    pub element: Vec<u8>,
    pub timestamp: Instant,
}

#[derive(Debug, Clone)]
pub enum OperationType {
    Add,
    Remove,
}

/// Metrics collected for a batch operation
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
        let total_latency_ms: f64 = latencies.iter().map(|d| d.as_secs_f64()).sum();
        let average_latency = Duration::from_secs_f64(total_latency_ms / latencies.len() as f64);
        let max_latency = *latencies.iter().max().unwrap_or(&Duration::ZERO);
        let min_latency = *latencies.iter().min().unwrap_or(&Duration::ZERO);
        let throughput = batch_size as f64 / total_duration.as_secs_f64();

        Self {
            batch_size,
            total_duration,
            average_latency,
            max_latency,
            min_latency,
            throughput,
            blockchain_confirmation_time: blockchain_time,
        }
    }
}

/// TPS (Transactions Per Second) limiter for blockchain simulation
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
        // Simulate blockchain confirmation time based on TPS
        Duration::from_secs_f64(num_transactions as f64 / self.max_tps as f64)
    }
}

/// Batch processor for handling batch operations with TPS limiting
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

    /// Process a batch of add operations
    pub fn process_add_batch(&mut self, issuer: &mut Issuer, elements: &[&[u8]]) -> BatchMetrics {
        let start_time = Instant::now();
        let mut latencies = Vec::new();

        debug!("Processing batch of {} add operations", elements.len());

        // Phase 1: Local computation - add all elements without TPS limiting
        for (i, element) in elements.iter().enumerate() {
            let op_start = Instant::now();

            // Perform local add operation (no blockchain interaction)
            issuer.add_elements(&[*element]);

            let op_duration = op_start.elapsed();
            latencies.push(op_duration);

            if (i + 1) % 100 == 0 || i == elements.len() - 1 {
                debug!(
                    "  Processed {}/{} add operations (local)",
                    i + 1,
                    elements.len()
                );
            }
        }

        // Phase 2: Single blockchain update for the entire batch
        let blockchain_start = Instant::now();
        self.tps_limiter.wait_for_next_transaction();
        let blockchain_duration = blockchain_start.elapsed();

        let total_duration = start_time.elapsed();
        let blockchain_confirmation_time = self.tps_limiter.get_blockchain_confirmation_time(1); // Only 1 transaction for the batch

        let metrics = BatchMetrics::new(
            elements.len(),
            total_duration,
            &latencies,
            blockchain_confirmation_time,
        );
        self.operation_latencies.extend(latencies);

        debug!(
            "Add batch completed: {:.2}s total ({:.2}s local + {:.2}s blockchain), {:.2} ops/s",
            total_duration.as_secs_f64(),
            (total_duration - blockchain_duration).as_secs_f64(),
            blockchain_duration.as_secs_f64(),
            metrics.throughput
        );

        metrics
    }

    /// Process a batch of remove operations
    pub fn process_remove_batch(
        &mut self,
        issuer: &mut Issuer,
        elements: &[&[u8]],
    ) -> BatchMetrics {
        let start_time = Instant::now();
        let mut latencies = Vec::new();

        debug!("Processing batch of {} remove operations", elements.len());

        // Phase 1: Local computation - remove all elements without TPS limiting
        for (i, element) in elements.iter().enumerate() {
            let op_start = Instant::now();

            // Perform local remove operation (no blockchain interaction)
            issuer.remove_elements(&[*element]);

            let op_duration = op_start.elapsed();
            latencies.push(op_duration);

            if (i + 1) % 100 == 0 || i == elements.len() - 1 {
                debug!(
                    "  Processed {}/{} remove operations (local)",
                    i + 1,
                    elements.len()
                );
            }
        }

        // Phase 2: Single blockchain update for the entire batch
        let blockchain_start = Instant::now();
        self.tps_limiter.wait_for_next_transaction();
        let blockchain_duration = blockchain_start.elapsed();

        let total_duration = start_time.elapsed();
        let blockchain_confirmation_time = self.tps_limiter.get_blockchain_confirmation_time(1); // Only 1 transaction for the batch

        let metrics = BatchMetrics::new(
            elements.len(),
            total_duration,
            &latencies,
            blockchain_confirmation_time,
        );
        self.operation_latencies.extend(latencies);

        debug!(
            "Remove batch completed: {:.2}s total ({:.2}s local + {:.2}s blockchain), {:.2} ops/s",
            total_duration.as_secs_f64(),
            (total_duration - blockchain_duration).as_secs_f64(),
            blockchain_duration.as_secs_f64(),
            metrics.throughput
        );

        metrics
    }

    /// Get statistics of all processed operations
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

/// Client verification simulator for concurrent testing
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

    /// Simulate concurrent verification operations
    pub fn verify_concurrent(
        &mut self,
        issuer: &Issuer,
        elements: &[Vec<u8>],
    ) -> (Duration, Duration, f64) {
        let start_time = Instant::now();
        let mut latencies = Vec::new();

        debug!(
            "Client {} starting verification of {} elements",
            self.client_id,
            elements.len()
        );

        for element in elements {
            let verify_start = Instant::now();

            // Generate proof and verify
            if let Some(proof) = issuer.generate_proof(element) {
                let verified = issuer.accumulator.verify_proof(element, &proof);
                assert!(
                    verified,
                    "Verification failed for client {}",
                    self.client_id
                );
            }

            let verify_duration = verify_start.elapsed();
            latencies.push(verify_duration);
        }

        let total_time = start_time.elapsed();
        let avg_latency = latencies.iter().sum::<Duration>() / latencies.len() as u32;
        let max_latency = *latencies.iter().max().unwrap_or(&Duration::ZERO);
        let throughput = elements.len() as f64 / total_time.as_secs_f64();

        self.verification_latencies.extend(latencies);

        info!(
            "Client {} verification: avg {:.2}ms, max {:.2}ms, {:.2} verifications/s",
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
        let throughput = total_verifications as f64
            / self
                .verification_latencies
                .iter()
                .map(|d| d.as_secs_f64())
                .sum::<f64>();

        (avg_latency, max_latency, throughput)
    }
}

/// Extensions to ExperimentResults for batch metrics
impl ExperimentResults {
    pub fn add_batch_metrics(&mut self, label: &str, _batch_index: usize, metrics: &BatchMetrics) {
        self.add_labeled_metric(
            label,
            metrics.batch_size,
            metrics.total_duration,
            Some(metrics.batch_size),
        );

        // Add detailed breakdown
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

        self.add_labeled_metric(
            &format!("{}-throughput", label),
            metrics.batch_size,
            Duration::from_secs_f64(1.0 / metrics.throughput), // Convert to duration for consistency
            Some(metrics.batch_size),
        );

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
            Some(1), // Per operation basis
        );

        self.add_labeled_metric(
            &format!("{}-verification_max", label),
            batch_size,
            max_latency,
            Some(1),
        );

        self.add_labeled_metric(
            &format!("{}-verification_throughput", label),
            batch_size,
            Duration::from_secs_f64(1.0 / throughput),
            Some(1),
        );
    }
}

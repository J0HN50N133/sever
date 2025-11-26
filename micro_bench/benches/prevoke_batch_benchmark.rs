// prevoke_batch_benchmark.rs

// This benchmark is adapted from batch_benchmark.rs to test the Prevoke scheme.
// It measures end-to-end latency and throughput for issuance, revocation, and verification.

pub mod common;
mod prevoke_batch_common;

use common::{generate_test_elements, ExperimentResults};
use log::{debug, info};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use crate::prevoke_batch_common::{BatchProcessor, PrevokeIssuer, VerificationClient};

const BLOCKCHAIN_TPS: u64 = 200;
const BATCH_SIZES: &[(usize, usize)] = &[
    (1, 10_000),
    (10, 10_000),
    (50, 10_000),
    (100, 100_000),
    (500, 100_000),
    (1000, 100_000),
];
const REVOCATION_PERCENTAGES: &[f64] = &[0.10, 0.25, 0.50];

/// Scenario S1: Issuance Testing - Add credentials in batches
fn run_s1_issuance_tests(results: &mut ExperimentResults) {
    info!("Starting S1: Issuance Testing for Prevoke");
    debug!("Testing batch sizes: {:?}", BATCH_SIZES);

    for &(batch_size, TOTAL_CREDENTIALS) in BATCH_SIZES {
        info!("Testing batch size: {}", batch_size);

        let elements = generate_test_elements(TOTAL_CREDENTIALS);
        let mut issuer = PrevokeIssuer::new();
        let mut batch_processor = BatchProcessor::new(BLOCKCHAIN_TPS);

        debug!("Starting S1 test for batch size {}", batch_size);
        let test_start = Instant::now();

        let mut batch_metrics = Vec::new();
        for chunk in elements.chunks(batch_size) {
            let metrics = batch_processor.process_add_batch(&mut issuer, chunk);
            batch_metrics.push(metrics);
        }

        let total_test_time = test_start.elapsed();
        let total_operations: usize = batch_metrics.iter().map(|m| m.batch_size).sum();
        let overall_throughput = total_operations as f64 / total_test_time.as_secs_f64();

        results.add_throughput_metric(
            "S1-Issuance-Prevoke",
            batch_size,
            overall_throughput,
            total_test_time,
        );

        if !batch_metrics.is_empty() {
            let avg_latencies: Vec<Duration> =
                batch_metrics.iter().map(|m| m.average_latency).collect();
            let total_batch_time = batch_metrics
                .iter()
                .map(|m| m.total_duration)
                .sum::<Duration>();

            results.add_batch_latency_metric(
                &format!("S1-Issuance-Prevoke-Batch"),
                batch_size,
                overall_throughput,
                total_batch_time,
                avg_latencies.iter().sum::<Duration>() / avg_latencies.len() as u32,
                batch_metrics
                    .iter()
                    .map(|m| m.max_latency)
                    .max()
                    .unwrap_or_default(),
                batch_metrics
                    .iter()
                    .map(|m| m.min_latency)
                    .min()
                    .unwrap_or_default(),
                batch_metrics
                    .iter()
                    .map(|m| m.blockchain_confirmation_time)
                    .sum(),
            );
        }
    }
}

/// Scenario S2: Revocation Testing - Revoke credentials in batches
fn run_s2_revocation_tests(results: &mut ExperimentResults) {
    info!("Starting S2: Revocation Testing for Prevoke");

    for &revocation_percentage in REVOCATION_PERCENTAGES {
        for &(batch_size, TOTAL_CREDENTIALS) in BATCH_SIZES {
            let revocation_count = (TOTAL_CREDENTIALS as f64 * revocation_percentage) as usize;
            let label_suffix = format!("{}%", (revocation_percentage * 100.0) as usize);

            info!(
                "Testing revocation: {} ({})",
                label_suffix, revocation_count
            );

            let elements = generate_test_elements(TOTAL_CREDENTIALS);
            let mut issuer = PrevokeIssuer::new();
            let indices: Vec<usize> = (0..TOTAL_CREDENTIALS).collect();
            issuer.add_elements(&elements);

            let indices_to_revoke = &indices[0..revocation_count];
            let mut batch_processor = BatchProcessor::new(BLOCKCHAIN_TPS);

            let test_start = Instant::now();
            let mut batch_metrics = Vec::new();
            for chunk in indices_to_revoke.chunks(batch_size) {
                let metrics = batch_processor.process_remove_batch(&mut issuer, chunk);
                batch_metrics.push(metrics);
            }
            let total_test_time = test_start.elapsed();
            let total_operations: usize = batch_metrics.iter().map(|m| m.batch_size).sum();
            let overall_throughput = total_operations as f64 / total_test_time.as_secs_f64();
            let revoke_scenario_name = format!("S2-Revoke{}-Prevoke", label_suffix);

            results.add_throughput_metric(
                &revoke_scenario_name,
                batch_size,
                overall_throughput,
                total_test_time,
            );

            if !batch_metrics.is_empty() {
                let avg_latencies: Vec<Duration> =
                    batch_metrics.iter().map(|m| m.average_latency).collect();
                let total_batch_time = batch_metrics
                    .iter()
                    .map(|m| m.total_duration)
                    .sum::<Duration>();

                results.add_batch_latency_metric(
                    &format!("{}-Batch", revoke_scenario_name),
                    batch_size,
                    overall_throughput,
                    total_batch_time,
                    avg_latencies.iter().sum::<Duration>() / avg_latencies.len() as u32,
                    batch_metrics
                        .iter()
                        .map(|m| m.max_latency)
                        .max()
                        .unwrap_or_default(),
                    batch_metrics
                        .iter()
                        .map(|m| m.min_latency)
                        .min()
                        .unwrap_or_default(),
                    batch_metrics
                        .iter()
                        .map(|m| m.blockchain_confirmation_time)
                        .sum(),
                );
            }
        }
    }
}

/// Scenario S3: Concurrent Verification Testing
fn run_s3_concurrent_verification_tests(results: &mut ExperimentResults) {
    info!("Starting S3: Concurrent Verification Testing for Prevoke");

    const NUM_CLIENTS: usize = 4;
    const VERIFICATION_INTERVAL_MS: u64 = 10;

    for &revocation_percentage in REVOCATION_PERCENTAGES {
        for &(batch_size, TOTAL_CREDENTIALS) in BATCH_SIZES {
            let revocation_count = (TOTAL_CREDENTIALS as f64 * revocation_percentage) as usize;
            let label_suffix = format!("{}%", (revocation_percentage * 100.0) as usize);

            let elements = generate_test_elements(TOTAL_CREDENTIALS);
            let mut issuer = PrevokeIssuer::new();
            issuer.add_elements(&elements);

            let indices_to_revoke: Vec<usize> = (0..revocation_count).collect();

            let verification_items: Vec<(Vec<u8>, usize)> = elements
                .iter()
                .skip(revocation_count)
                .cloned()
                .zip(revocation_count..TOTAL_CREDENTIALS)
                .collect();

            let issuer_arc = Arc::new(std::sync::Mutex::new(issuer));
            let mut client_handles = Vec::new();

            for client_id in 0..NUM_CLIENTS {
                let issuer_clone = Arc::clone(&issuer_arc);
                let items_clone = verification_items.clone();
                let handle = thread::spawn(move || {
                    let mut client = VerificationClient::new(client_id);
                    let mut verifications_performed = 0;
                    loop {
                        let sample_size = 100.min(items_clone.len());
                        let sample = &items_clone[0..sample_size];

                        {
                            let issuer_guard = issuer_clone.lock().unwrap();
                            client.verify_concurrent(&issuer_guard, sample);
                        }

                        verifications_performed += 100;
                        if verifications_performed >= 1000 {
                            break;
                        }
                        thread::sleep(std::time::Duration::from_millis(VERIFICATION_INTERVAL_MS));
                    }
                    client.get_verification_stats()
                });
                client_handles.push(handle);
            }

            let revocation_start = Instant::now();
            let mut batch_processor = BatchProcessor::new(BLOCKCHAIN_TPS);
            for chunk in indices_to_revoke.chunks(batch_size) {
                let mut issuer_guard = issuer_arc.lock().unwrap();
                batch_processor.process_remove_batch(&mut issuer_guard, chunk);
            }
            let revocation_duration = revocation_start.elapsed();

            let mut verification_stats = Vec::new();
            for handle in client_handles {
                verification_stats.push(handle.join().unwrap());
            }

            let total_avg_latency_sum: Duration =
                verification_stats.iter().map(|(avg, _, _)| *avg).sum();
            let overall_avg_latency = total_avg_latency_sum / verification_stats.len() as u32;
            let overall_max_latency = verification_stats
                .iter()
                .map(|(_, max, _)| *max)
                .max()
                .unwrap_or_default();
            let overall_throughput: f64 = verification_stats.iter().map(|(_, _, tp)| *tp).sum();

            let concurrent_scenario_name = format!("S3-Concurrent-Prevoke-{}", label_suffix);
            results.add_batch_latency_metric(
                &format!("{}-Verification", concurrent_scenario_name),
                batch_size,
                overall_throughput,
                revocation_duration,
                overall_avg_latency,
                overall_max_latency,
                Duration::ZERO,
                Duration::ZERO,
            );
        }
    }
}

fn main() {
    logforth::starter_log::stdout().apply();
    info!("Starting Prevoke Batch Performance Benchmarks...");

    let mut results = ExperimentResults::new("prevoke_secure_batch_performance");
    let mut verification_results = ExperimentResults::new("prevoke_secure_concurrent_verification");

    run_s1_issuance_tests(&mut results);
    run_s2_revocation_tests(&mut results);
    run_s3_concurrent_verification_tests(&mut verification_results);

    results
        .save_to_file("prevoke_batch_benchmark_results.json")
        .unwrap();
    verification_results
        .save_to_file("prevoke_verification_results.json")
        .unwrap();

    info!("Prevoke benchmark results saved.");
}

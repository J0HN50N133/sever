mod batch_common;
pub mod common;

use batch_common::{BatchProcessor, VerificationClient};
use common::{generate_test_elements, to_element_slices, ExperimentResults, Issuer};
use log::{debug, info};
use minchash::MultisetHash; // Keep MultisetHash trait for type bounds
use minchash::SecureMultisetHash; // Keep SecureMultisetHash for type
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

const TOTAL_CREDENTIALS: usize = 10_000;
const BLOCKCHAIN_TPS: u64 = 500;
const BATCH_SIZES: &[usize] = &[1, 10, 50, 100, 500, 1000];
const REVOCATION_PERCENTAGES: &[f64] = &[0.10, 0.25, 0.50];

/// Scenario S1: Issuance Testing - Add credentials in batches
fn run_s1_issuance_tests(results: &mut ExperimentResults) {
    info!("Starting S1: Issuance Testing");
    debug!("Testing batch sizes: {:?}", BATCH_SIZES);

    for &batch_size in BATCH_SIZES {
        info!("Testing batch size: {}", batch_size);

        // Prepare test data
        let elements = generate_test_elements(TOTAL_CREDENTIALS);
        let element_slices = to_element_slices(&elements);

        let mut issuer = Issuer::new();
        let mut batch_processor = BatchProcessor::new(BLOCKCHAIN_TPS);

        debug!("Starting S1 test for batch size {}", batch_size);
        let test_start = Instant::now();

        // Process in batches
        let mut batch_metrics = Vec::new();
        for (batch_num, chunk) in element_slices.chunks(batch_size).enumerate() {
            debug!("Processing batch {} of size {}", batch_num + 1, chunk.len());
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

        if !batch_metrics.is_empty() {
            let avg_latencies: Vec<Duration> =
                batch_metrics.iter().map(|m| m.average_latency).collect();
            let total_batch_time = batch_metrics
                .iter()
                .map(|m| m.total_duration)
                .sum::<Duration>();

            results.add_batch_latency_metric(
                &format!("S1-Issuance-Batch"),
                batch_size,
                overall_throughput,
                total_batch_time,
                avg_latencies.iter().sum::<Duration>() / avg_latencies.len() as u32,
                batch_metrics.iter().map(|m| m.max_latency).max().unwrap_or_default(),
                batch_metrics.iter().map(|m| m.min_latency).min().unwrap_or_default(),
                batch_metrics.iter().map(|m| m.blockchain_confirmation_time).sum(),
            );
        }
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
                    batch_metrics.iter().map(|m| m.max_latency).max().unwrap_or_default(),
                    batch_metrics.iter().map(|m| m.min_latency).min().unwrap_or_default(),
                    batch_metrics.iter().map(|m| m.blockchain_confirmation_time).sum(),
                );
            }
        }
    }
}

/// Scenario S3: Concurrent Verification Testing (Unrealistic Best-Case)
fn run_s3_concurrent_verification_tests(results: &mut ExperimentResults) {
    info!("Starting S3: Concurrent Verification Testing");

    const NUM_CLIENTS: usize = 4;
    const VERIFICATION_INTERVAL_MS: u64 = 10;

    for &revocation_percentage in REVOCATION_PERCENTAGES {
        let revocation_count = (TOTAL_CREDENTIALS as f64 * revocation_percentage) as usize;
        let label_suffix = format!("{}%", (revocation_percentage * 100.0) as usize);

        for &batch_size in BATCH_SIZES {
            let elements = generate_test_elements(TOTAL_CREDENTIALS);
            let element_slices = to_element_slices(&elements);
            let mut issuer = Issuer::new();
            issuer.add_elements(&element_slices);

            let elements_to_revoke: Vec<Vec<u8>> = element_slices[0..revocation_count]
                .iter().map(|&s| s.to_vec()).collect();
            let verification_elements: Vec<Vec<u8>> = element_slices[revocation_count..]
                .iter().map(|&s| s.to_vec()).collect();

            let issuer_arc = Arc::new(std::sync::Mutex::new(issuer));
            let mut client_handles = Vec::new();

            for client_id in 0..NUM_CLIENTS {
                let issuer_clone = Arc::clone(&issuer_arc);
                let elements_clone = verification_elements.clone();
                let handle = thread::spawn(move || {
                    let mut client = VerificationClient::new(client_id);
                    let mut verifications_performed = 0;
                    loop {
                        let (_avg_latency, _max_latency, _throughput) = {
                            let issuer_guard = issuer_clone.lock().unwrap();
                            let sample_size = 100.min(elements_clone.len());
                            let sample = &elements_clone[0..sample_size];
                            client.verify_concurrent(&issuer_guard, sample)
                        };
                        verifications_performed += 100;
                        if verifications_performed >= 1000 { break; }
                        thread::sleep(std::time::Duration::from_millis(VERIFICATION_INTERVAL_MS));
                    }
                    client.get_verification_stats()
                });
                client_handles.push(handle);
            }

            let revocation_start = Instant::now();
            let mut batch_processor = BatchProcessor::new(BLOCKCHAIN_TPS);
            for chunk in elements_to_revoke.chunks(batch_size) {
                let mut issuer_guard = issuer_arc.lock().unwrap();
                let chunk_refs: Vec<&[u8]> = chunk.iter().map(|vec| vec.as_slice()).collect();
                batch_processor.process_remove_batch(&mut issuer_guard, &chunk_refs);
            }
            let revocation_duration = revocation_start.elapsed();

            let mut verification_stats = Vec::new();
            for handle in client_handles {
                verification_stats.push(handle.join().unwrap());
            }

            let total_avg_latency_sum: Duration = verification_stats.iter().map(|(avg, _, _)| *avg).sum();
            let overall_avg_latency = total_avg_latency_sum / verification_stats.len() as u32;
            let overall_max_latency = verification_stats.iter().map(|(_, max, _)| *max).max().unwrap_or_default();
            let overall_throughput: f64 = verification_stats.iter().map(|(_, _, tp)| *tp).sum();

            let concurrent_scenario_name = format!("S3-Concurrent-{}", label_suffix);
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

/// Scenario S4: Realistic Witness Update Verification
fn run_s4_realistic_verification_tests(results: &mut ExperimentResults) {
    info!("Starting S4: Realistic Witness Update Verification");

    for &revocation_percentage in REVOCATION_PERCENTAGES {
        let revocation_count = (TOTAL_CREDENTIALS as f64 * revocation_percentage) as usize;
        let label_suffix = format!("{}%", (revocation_percentage * 100.0) as usize);
        info!("Testing with {} revocation", label_suffix);

        let elements = generate_test_elements(TOTAL_CREDENTIALS);
        let element_slices = to_element_slices(&elements);

        let mut issuer = Issuer::new();
        issuer.add_elements(&element_slices[..revocation_count]); // Add elements that will be revoked
        let _initial_root = issuer.accumulator.clone();

        // Create proofs for elements that will NOT be revoked, based on the initial state
        let verification_elements_for_proofs: Vec<Vec<u8>> = element_slices[revocation_count..]
            .iter().map(|&s| s.to_vec()).collect();
        let mut client_proofs = Vec::new();
        for elem in &verification_elements_for_proofs {
            if let Some(proof) = issuer.generate_proof(elem) {
                client_proofs.push((elem.clone(), proof)); // Store element and its proof
            }
        }

        // Calculate delta by hashing the added elements
        let mut delta_acc = SecureMultisetHash::new();
        delta_acc.add_elements(&to_element_slices(&verification_elements_for_proofs));
        let delta = delta_acc;

        // Add the rest of the elements to the issuer to create the 'current' state
        issuer.add_elements(&to_element_slices(&verification_elements_for_proofs));
        let current_root = issuer.accumulator.clone();

        // Simulate witness update
        let mut latencies = Vec::new();
        let test_start = Instant::now();

        for (element, old_proof) in &client_proofs {
            let op_start = Instant::now();

            // 1. Client updates its proof using the delta
            let new_proof = SecureMultisetHash { current: old_proof.current + delta.current };

            // 2. Client verifies the new proof against the current root
            let is_valid = current_root.verify_proof(element, &new_proof);
            assert!(is_valid);

            latencies.push(op_start.elapsed());
        }
        let total_duration = test_start.elapsed();

        let avg_latency = latencies.iter().sum::<Duration>() / latencies.len() as u32;
        let throughput = client_proofs.len() as f64 / total_duration.as_secs_f64();
        
        let scenario_name = format!("S4-Witness-Update-Verification_{}", label_suffix);
        results.add_latency_metric(
            &scenario_name,
            revocation_count, // Use revocation count as the "size" parameter
            avg_latency,
            throughput,
            total_duration,
        );
    }
}


fn main() {
    logforth::starter_log::stdout().apply();
    info!("Starting Batch Performance Benchmarks...");

    let mut results = ExperimentResults::new("minchash_secure_batch_performance");
    let verification_results = ExperimentResults::new("minchash_secure_concurrent_verification");

    // run_s1_issuance_tests(&mut results);
    // run_s2_revocation_tests(&mut results);
    // run_s3_concurrent_verification_tests(&mut verification_results);
    run_s4_realistic_verification_tests(&mut results); // Run the new realistic benchmark

    // Save results
    results.save_to_file("batch_benchmark_results.json").unwrap();
    verification_results.save_to_file("verification_results.json").unwrap();
    
    info!("Benchmark results saved.");
}
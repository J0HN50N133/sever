mod common;

use common::{Issuer, Client, ExperimentResults, generate_test_elements, to_element_slices, print_summary};
use rayon::prelude::*;
use std::hint;
use std::time::Instant;

fn run_client_verification_benchmarks(results: &mut ExperimentResults) {
    println!("Running client verification benchmarks...");

    let sizes = vec![10_000, 50_000, 500_000, 1_000_000]; // Match issuer test sizes

    for s in sizes {
        println!("Testing verification with {} elements", s);

        // Setup: Create issuer with elements and generate proofs
        let elements = generate_test_elements(s);
        let element_slices = to_element_slices(&elements);

        let mut issuer = Issuer::new();
        issuer.add_elements(&element_slices);
        let root = issuer.get_root();

        // Generate proofs for all elements
        let proofs: Vec<_> = element_slices
            .iter()
            .map(|e| issuer.generate_proof(e))
            .collect();

        // 1. Single verification performance - measure average latency
        println!("  - Running single verification latency test for {} elements", s);
        let test_element = &element_slices[0];
        let test_proof = &proofs[0];

        let start = Instant::now();
        for _ in 0..10_000 {
            // Run many iterations for accurate latency measurement
            let client = Client::new(
                hint::black_box(test_element),
                hint::black_box(test_proof.clone()),
            );
            client.verify_proof(hint::black_box(&root));
        }
        let single_verification_duration = start.elapsed();
        let avg_latency_us = (single_verification_duration.as_nanos() / 10_000) as f64 / 1000.0; // microseconds

        results.add_labeled_metric(
            "Client.Verification.Latency",
            s,
            single_verification_duration,
            Some(10_000)
        );
        println!(
            "    ✓ Single verification completed. Average latency: {:.2} μs",
            avg_latency_us
        );

        // 2. Concurrent verification throughput - test with different thread counts
        println!("  - Running concurrent verification throughput test for {} elements", s);
        let start = Instant::now();
        let verified_count: usize = element_slices
            .par_iter()
            .zip(proofs.par_iter())
            .map(|(element, proof)| {
                let client = Client::new(element, proof.clone());
                client.verify_proof(&root) as usize
            })
            .sum();
        let concurrent_throughput_duration = start.elapsed();
        let throughput_ops_per_sec = verified_count as f64 / concurrent_throughput_duration.as_secs_f64();

        results.add_labeled_metric(
            "Client.Verification.Throughput",
            s,
            concurrent_throughput_duration,
            Some(verified_count)
        );
        println!(
            "    ✓ Concurrent verification completed. Throughput: {:.2} ops/sec",
            throughput_ops_per_sec
        );

        // 3. Test verification latency impact of set size
        // Verify that verification time is constant regardless of set size
        println!("  - Testing verification latency vs set size for {} elements", s);
        let mut latencies = Vec::new();

        // Test verification of different elements to ensure consistency
        for i in 0..100.min(s) {
            let test_element = &element_slices[i];
            let test_proof = &proofs[i];

            let start = Instant::now();
            let client = Client::new(test_element, test_proof.clone());
            client.verify_proof(&root);
            let latency = start.elapsed();
            latencies.push(latency.as_nanos() as f64 / 1000.0); // microseconds
        }

        let avg_latency = latencies.iter().sum::<f64>() / latencies.len() as f64;
        let max_latency = latencies.iter().fold(0.0_f64, |a, &b| a.max(b));
        let min_latency = latencies.iter().fold(f64::INFINITY, |a, &b| a.min(b));

        println!(
            "    ✓ Verification latency analysis: avg={:.2}μs, min={:.2}μs, max={:.2}μs",
            avg_latency, min_latency, max_latency
        );
    }
}

fn main() {
    println!("Starting Client Verification Benchmarks...");
    let experiment_start = Instant::now();

    let mut results = ExperimentResults::new("minchash_secure_client_performance");

    // Run client verification benchmarks
    run_client_verification_benchmarks(&mut results);

    // Record total experiment duration
    let total_duration = experiment_start.elapsed();
    results.set_total_duration(total_duration);

    // Save results to JSON file
    let output_file = "client_benchmark_results.json";
    match results.save_to_file(output_file) {
        Ok(()) => {
            println!("Client verification benchmark results saved to {}", output_file);
            print_summary(&results);
        }
        Err(e) => {
            eprintln!("Failed to save results: {}", e);
        }
    }
}
mod common;

use common::{Issuer, ExperimentResults, generate_test_elements, to_element_slices, print_summary};
use std::hint;
use std::time::Instant;

fn run_issuer_local_benchmarks(results: &mut ExperimentResults) {
    println!("Running issuer local benchmarks...");

    let sizes = vec![10_000, 50_000, 500_000, 1_000_000];

    for s in sizes {
        println!("Testing with {} elements", s);

        // Prepare test data
        let elements = generate_test_elements(s);
        let element_slices = to_element_slices(&elements);

        // 1. Acc.Generation: 测量生成新的 Accumulator 所需时间
        println!("  - Running accumulator generation for {} elements", s);
        let start = Instant::now();
        {
            let mut issuer = Issuer::new();
            issuer.add_elements(hint::black_box(&element_slices));
        }
        let acc_generation_duration = start.elapsed();
        results.add_labeled_metric("Acc.Generation", s, acc_generation_duration, Some(s));
        println!(
            "    ✓ Accumulator generation completed in {:.2} ms",
            acc_generation_duration.as_secs_f64() * 1000.0
        );

        // Setup for other benchmarks: create an issuer with s elements
        let mut issuer_with_elements = Issuer::new();
        issuer_with_elements.add_elements(&element_slices);

        // 2. Proof.Generation: 测量生成证明所需总时间
        println!("  - Running proof generation for {} elements", s);
        let start = Instant::now();
        issuer_with_elements.generate_proofs_bulk(hint::black_box(&element_slices));
        let proof_generation_duration = start.elapsed();
        results.add_labeled_metric("Proof.Generation", s, proof_generation_duration, Some(s));
        println!(
            "    ✓ Proof generation completed in {:.2} ms",
            proof_generation_duration.as_secs_f64() * 1000.0
        );

        // Setup for removal benchmarks with different percentages
        let removal_10_percent = (s as f64 * 0.10) as usize;
        let removal_25_percent = (s as f64 * 0.25) as usize;
        let removal_50_percent = (s as f64 * 0.50) as usize;

        let elements_to_remove_10 = &element_slices[0..removal_10_percent];
        let elements_to_remove_25 = &element_slices[0..removal_25_percent];
        let elements_to_remove_50 = &element_slices[0..removal_50_percent];

        // 3. Acc.Revoke, 10%: 撤销10%的证书 + 重新生成证明所需时间
        let mut issuer_after_removal_10 = issuer_with_elements.clone();
        println!(
            "  - Running 10% revoke + proof regeneration for {} elements",
            s
        );
        let start = Instant::now();
        issuer_after_removal_10.remove_elements(hint::black_box(elements_to_remove_10));
        let elements_to_keep_90 = &element_slices[removal_10_percent..];
        issuer_after_removal_10.generate_proofs_bulk(hint::black_box(elements_to_keep_90));
        let revoke_10_duration = start.elapsed();
        results.add_labeled_metric(
            "Acc.Revoke, 10%",
            s,
            revoke_10_duration,
            Some(elements_to_keep_90.len()),
        );
        println!(
            "    ✓ 10% revoke + proof regeneration completed in {:.2} ms",
            revoke_10_duration.as_secs_f64() * 1000.0
        );

        // 4. Acc.Revoke, 25%: 撤销25%的证书 + 重新生成证明所需时间
        let mut issuer_after_removal_25 = issuer_with_elements.clone();
        println!(
            "  - Running 25% revoke + proof regeneration for {} elements",
            s
        );
        let start = Instant::now();
        issuer_after_removal_25.remove_elements(hint::black_box(elements_to_remove_25));
        let elements_to_keep_75 = &element_slices[removal_25_percent..];
        issuer_after_removal_25.generate_proofs_bulk(hint::black_box(elements_to_keep_75));
        let revoke_25_duration = start.elapsed();
        results.add_labeled_metric(
            "Acc.Revoke, 25%",
            s,
            revoke_25_duration,
            Some(elements_to_keep_75.len()),
        );
        println!(
            "    ✓ 25% revoke + proof regeneration completed in {:.2} ms",
            revoke_25_duration.as_secs_f64() * 1000.0
        );

        // 5. Acc.Revoke, 50%: 撤销50%的证书 + 重新生成证明所需时间
        let mut issuer_after_removal_50 = issuer_with_elements.clone();
        println!(
            "  - Running 50% revoke + proof regeneration for {} elements",
            s
        );
        let start = Instant::now();
        issuer_after_removal_50.remove_elements(hint::black_box(elements_to_remove_50));
        let elements_to_keep_50 = &element_slices[removal_50_percent..];
        issuer_after_removal_50.generate_proofs_bulk(hint::black_box(elements_to_keep_50));
        let revoke_50_duration = start.elapsed();
        results.add_labeled_metric(
            "Acc.Revoke, 50%",
            s,
            revoke_50_duration,
            Some(elements_to_keep_50.len()),
        );
        println!(
            "    ✓ 50% revoke + proof regeneration completed in {:.2} ms",
            revoke_50_duration.as_secs_f64() * 1000.0
        );
    }
}

fn main() {
    println!("Starting Issuer Performance Benchmarks...");
    let experiment_start = Instant::now();

    let mut results = ExperimentResults::new("minchash_secure_issuer_performance");

    // Run issuer benchmarks
    run_issuer_local_benchmarks(&mut results);

    // Record total experiment duration
    let total_duration = experiment_start.elapsed();
    results.set_total_duration(total_duration);

    // Save results to JSON file
    let output_file = "issuer_benchmark_results.json";
    match results.save_to_file(output_file) {
        Ok(()) => {
            println!("Issuer benchmark results saved to {}", output_file);
            print_summary(&results);
        }
        Err(e) => {
            eprintln!("Failed to save results: {}", e);
        }
    }
}
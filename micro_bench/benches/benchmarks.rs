mod common;

use std::time::Instant;

fn main() {
    println!("Running All Minchash Performance Benchmarks...");
    let experiment_start = Instant::now();

    // Run individual benchmarks - they save their own results
    println!("Running issuer benchmarks...");
    let issuer_status = std::process::Command::new("cargo")
        .args(&["bench", "--bench", "issuer_benchmark"])
        .status()
        .expect("Failed to run cargo bench command for issuer");

    if issuer_status.success() {
        println!("✓ Issuer benchmarks completed successfully");
        println!("  Results saved to: issuer_benchmark_results.json");

        // Load and print issuer summary
        if let Ok(issuer_data) = std::fs::read_to_string("issuer_benchmark_results.json") {
            if let Ok(issuer_results) = serde_json::from_str::<common::ExperimentResults>(&issuer_data) {
                println!("  Issuer benchmark time: {:.2} ms", issuer_results.total_duration_ms);
            }
        }
    } else {
        eprintln!("✗ Failed to run issuer benchmarks");
    }

    println!();

    println!("Running client verification benchmarks...");
    let client_status = std::process::Command::new("cargo")
        .args(&["bench", "--bench", "client_benchmark"])
        .status()
        .expect("Failed to run cargo bench command for client");

    if client_status.success() {
        println!("✓ Client verification benchmarks completed successfully");
        println!("  Results saved to: client_benchmark_results.json");

        // Load and print client summary
        if let Ok(client_data) = std::fs::read_to_string("client_benchmark_results.json") {
            if let Ok(client_results) = serde_json::from_str::<common::ExperimentResults>(&client_data) {
                println!("  Client benchmark time: {:.2} ms", client_results.total_duration_ms);
            }
        }
    } else {
        eprintln!("✗ Failed to run client verification benchmarks");
    }

    println!();

    println!("Running batch processing benchmarks...");
    let batch_status = std::process::Command::new("cargo")
        .args(&["bench", "--bench", "batch_benchmark"])
        .status()
        .expect("Failed to run cargo bench command for batch");

    if batch_status.success() {
        println!("✓ Batch processing benchmarks completed successfully");
        println!("  Results saved to: batch_benchmark_results.json");

        // Load and print batch summary
        if let Ok(batch_data) = std::fs::read_to_string("batch_benchmark_results.json") {
            if let Ok(batch_results) = serde_json::from_str::<common::ExperimentResults>(&batch_data) {
                println!("  Batch benchmark time: {:.2} ms", batch_results.total_duration_ms);
            }
        }
    } else {
        eprintln!("✗ Failed to run batch processing benchmarks");
    }

    println!();

    // Record total time
    let total_duration = experiment_start.elapsed();
    println!("All benchmarks completed!");
    println!("Total benchmark suite execution time: {:.2} s", total_duration.as_secs_f64());
    println!();
    println!("Result files:");
    println!("  - Issuer benchmarks: issuer_benchmark_results.json");
    println!("  - Client benchmarks: client_benchmark_results.json");
    println!("  - Batch benchmarks: batch_benchmark_results.json");
    println!();
    println!("You can run individual benchmarks with:");
    println!("  cargo bench --bench issuer_benchmark");
    println!("  cargo bench --bench client_benchmark");
    println!("  cargo bench --bench batch_benchmark");
}

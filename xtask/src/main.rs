use std::env;
use anyhow::Result;
use xshell::{Shell, cmd};

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    let task = args.get(1).map(|s| s.as_str()).unwrap_or("default");

    match task {
        "run" => run_experiment(),
        _ => {
            println!("Usage: cargo xtask [run]");
            Ok(())
        }
    }
}

fn run_experiment() -> Result<()> {
    let sh = Shell::new()?;
    
    // 1. Build all necessary binaries in release mode
    println!("Building binaries...");
    cmd!(sh, "cargo build --release --workspace").run()?;
    println!("Build complete.");

    // Define paths to the binaries
    let target_dir = sh.current_dir().join("target/release");
    let blockchain_server_path = target_dir.join("blockchain_server");
    let issuer_server_path = target_dir.join("issuer_server");
    let simulation_path = target_dir.join("simulation");

    // 2. Run the services in the background
    println!("Starting background services...");

    let blockchain_server = duct::cmd(&blockchain_server_path, &[] as &[&str])
        .stdout_capture()
        .stderr_capture()
        .start()?;
    
    let issuer_server = duct::cmd(&issuer_server_path, &[] as &[&str])
        .stdout_capture()
        .stderr_capture()
        .start()?;

    // Use a guard to ensure servers are killed on panic or early return
    let _server_guard = ServerGuard {
        blockchain: blockchain_server,
        issuer: issuer_server,
    };

    // 3. Wait for services to initialize
    println!("Waiting for services to start...");
    std::thread::sleep(std::time::Duration::from_secs(3));
    println!("Services started.");

    // 4. Run the simulation client in the foreground
    println!("Running experiment simulation...");
    let simulation_output = duct::cmd(&simulation_path, &[] as &[&str])
        .stdout_capture()
        .stderr_capture()
        .read()?;

    println!("--- Simulation Output ---");
    println!("{}", simulation_output);
    println!("--- End of Simulation ---");
    
    // 5. Cleanup is handled by the ServerGuard's Drop implementation
    println!("Experiment finished. Cleaning up background services...");

    Ok(())
}

/// A guard to ensure background processes are killed when this struct is dropped.
struct ServerGuard {
    blockchain: duct::Handle,
    issuer: duct::Handle,
}

impl Drop for ServerGuard {
    fn drop(&mut self) {
        println!("Terminating background services...");
        if let Err(e) = self.blockchain.kill() {
            eprintln!("Failed to kill blockchain_server: {}", e);
        }
        if let Err(e) = self.issuer.kill() {
            eprintln!("Failed to kill issuer_server: {}", e);
        }
        println!("Cleanup complete.");
    }
}

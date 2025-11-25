# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Minchash is a Rust library implementing order-independent hash functions for multisets using elliptic curve cryptography. The library provides two main implementations:

- **SecureMultisetHash**: Cryptographically secure implementation using elliptic curve group operations (k256)
- **FastMultisetHash**: High-performance implementation using 256-bit integer arithmetic

Both implementations support incremental addition/removal of elements and bulk operations with parallel processing.

## Development Commands

### Building and Testing
```bash
# Build the project
cargo build

# Run tests
cargo test

# Run tests with output
cargo test -- --nocapture

# Run specific test
cargo test test_basic
cargo test test_proofs
```

### Benchmarking
```bash
# Run all benchmarks
cargo bench

# Run specific benchmark
cargo bench -- "Secure add elements individually"
cargo bench -- "Fast add elements in parallel"
```

### Code Quality
```bash
# Check code formatting
cargo fmt --check

# Format code
cargo fmt

# Run clippy lints
cargo clippy

# Run clippy with all features
cargo clippy --all-features
```

## Architecture

### Core Components

1. **MultisetHash Trait** (`src/lib.rs`): Defines the common interface for all implementations
   - Core operations: `add()`, `remove()`, `add_elements()`, `remove_elements()`
   - State queries: `get_compressed()`, `get_digest()`
   - Cryptographic proofs: `generate_proof()`, `verify_proof()`
   - Serialization: `from_compressed()`

2. **SecureMultisetHash** (`src/secure.rs`):
   - Uses blake3 hash + k256 elliptic curve group operations
   - Maps elements to EC points using: `H(element) * GENERATOR`
   - Provides cryptographic security with performance trade-offs

3. **FastMultisetHash** (`src/fast.rs`):
   - Uses custom 256-bit hash function with FNV-inspired constants
   - Operates on four 64-bit integers for maximum performance
   - Suitable for non-security critical applications

### Key Design Patterns

- **Parallel Processing**: Both implementations use Rayon for parallel bulk operations
- **Incremental Updates**: Hash state can be updated without full recomputation
- **Order Independence**: Element order doesn't affect the final hash value
- **Proof System**: Both support generating/verifying proofs of element inclusion

### Testing Strategy

The test suite includes:
- Generic trait-based tests that run on both implementations
- Permutation invariance tests (verifying order independence)
- Parallel consistency tests
- Intensive stress tests (10M+ operations)
- Collision resistance tests
- Proof verification tests

## Dependencies

### Runtime Dependencies
- `k256`: Elliptic curve cryptography for secure implementation
- `blake3`: Fast cryptographic hash function
- `rayon`: Parallel processing framework
- `hex`: Hexadecimal encoding/decoding

### Development Dependencies
- `itertools`: Iterator utilities for testing
- `rand`: Random number generation for tests
- `criterion`: Benchmarking framework

## Usage Notes

- The library currently accepts byte slices as input elements
- Empty multisets return `None` for `get_compressed()` and `get_digest()`
- Both implementations are fully thread-safe for read operations
- For security-critical applications, use `SecureMultisetHash`
- For performance-critical applications, consider `FastMultisetHash`

## Performance Characteristics

Benchmarking shows significant performance differences:
- `FastMultisetHash`: Optimized for speed, can handle 10M+ elements efficiently
- `SecureMultisetHash`: Cryptographically secure but slower due to EC operations

Both implementations benefit greatly from parallel bulk operations when processing large datasets.
- benchmark只关心 secure 实现, fast 不管
mod common;

use common::{generate_test_elements, print_summary, to_element_slices, ExperimentResults};
use rand::{rng, Rng};
use rs_merkle::{algorithms::Sha256, MerkleTree};
use sha2::{Digest, Sha256 as Sha256Hasher};
use std::hint;
use std::time::Instant;

// 模拟 Prevoke 的 Issuer
#[derive(Clone)]
pub struct PrevokeIssuer {
    // 使用 rs_merkle 的标准树，泛型参数为 Sha256
    tree: Option<MerkleTree<Sha256>>,
    // 本地维护叶子节点列表，用于重建树
    leaves: Vec<[u8; 32]>,

    // Bloom Filter 相关
    bloom_filter: Vec<u8>,
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
        // 参数参考论文: m, k 的计算基于 n 和 p
        // 这里为了 Benchmark 使用固定大小 (例如 20M bits)
        let m = 20_000_000;
        let k = 7;

        PrevokeIssuer {
            tree: None,
            leaves: Vec::new(),
            bloom_filter: vec![0; (m + 7) / 8],
            m,
            k,
        }
    }

    // --- 1. Accumulator Generation ---
    // 对应论文：初始化并将 H(vcID) 插入 MTAcc
    pub fn add_element(&mut self, elements: &[Vec<u8>]) -> usize {
        // 1. 计算新元素的 Hash (Sha256)
        let mut leaves: Vec<_> = elements
            .iter()
            .map(|element| {
                let mut h = Sha256Hasher::new();
                h.update(element);
                let leaf_hash: [u8; 32] = h.finalize().into();
                leaf_hash
            })
            .collect();

        // 2. 添加到叶子列表
        let index = self.leaves.len();
        self.leaves.extend(&leaves);
        if let Some(tree) = &mut self.tree {
            tree.append(&mut leaves).commit();
        } else {
            // 3. 重建整个 Merkle Tree (rs_merkle 是 immutable 的)
            self.tree = Some(MerkleTree::<Sha256>::from_leaves(&self.leaves));
        }

        // 4. 返回新元素的索引
        index
    }

    // --- 2. Proof Generation ---
    // 对应论文：持有者获得 Witness [cite: 194]
    pub fn generate_proofs_bulk(&self, elements_count: usize) {
        let tree = self.tree.as_ref().expect("Tree not initialized");

        // Prevoke 中，每个用户需要自己的 Witness (Merkle Path)
        // 我们遍历所有索引生成 Proof
        for i in 0..elements_count {
            // rs_merkle 生成单个叶子的证明
            tree.proof(&[i]);
        }
    }

    // --- 3. Revocation ---
    // 对应论文 Algorithm 3:
    // 1. 更新 Bloom Filter
    // 2. 将 MTAcc 对应位置替换为随机 Hash
    // 3. 更新 MTAcc (Root)
    pub fn revoke_element(&mut self, elements: &[&[u8]], indices: &[usize]) {
        // 1. 更新 Bloom Filter (计算 k 个 hash 并置位)
        let n = elements.len();
        assert_eq!(n, indices.len());
        for i in 0..n {
            let element = &elements[i];
            let element_index = indices[i];
            let indexes = self.get_bloom_indexes(element);
            for idx in indexes {
                let byte_idx = idx / 8;
                let bit_idx = idx % 8;
                self.bloom_filter[byte_idx] |= 1 << bit_idx;
            }

            // 2. 更新 Merkle Tree Leaves
            // Prevoke 是"替换"而不是"删除"，这保持树的高度不变
            let mut rng = rng();
            let mut random_hash = [0u8; 32];
            rng.fill(&mut random_hash);
            self.leaves[element_index] = random_hash;
        }

        // 3. 重建 Merkle Tree 以获取新 Root
        // rs_merkle 这种 immutable 库通常通过重新 from_leaves 构建最快
        self.tree = Some(MerkleTree::<Sha256>::from_leaves(&self.leaves));
    }

    // --- Helper: Bloom Filter Indexes ---
    fn get_bloom_indexes(&self, item: &[u8]) -> Vec<usize> {
        let mut indexes = Vec::with_capacity(self.k);
        // 模拟 Double Hashing 生成 k 个索引
        let mut h = Sha256Hasher::new();
        h.update(item);
        let hash1 = h.finalize();
        let h1 = u64::from_be_bytes(hash1[0..8].try_into().unwrap());

        let mut h = Sha256Hasher::new();
        h.update(hash1);
        let hash2 = h.finalize();
        let h2 = u64::from_be_bytes(hash2[0..8].try_into().unwrap());

        for i in 0..self.k {
            let idx = (h1.wrapping_add((i as u64).wrapping_mul(h2))) as usize % self.m;
            indexes.push(idx);
        }
        indexes
    }
}

fn run_prevoke_issuer_benchmarks(results: &mut ExperimentResults) {
    println!("Running Prevoke (rs_merkle) Issuer benchmarks...");

    // 测试规模
    let sizes = vec![10_000, 50_000, 500_000, 1_000_000];
    let batch_size = 100;

    for s in sizes {
        println!("Testing with {} elements", s);

        let elements = generate_test_elements(s);
        let element_slices = to_element_slices(&elements);

        // --- 1. Acc.Generation (Individual Issuance) ---
        println!("  - [Acc.Gen] Issuing {} credentials one by one...", s);
        let start = Instant::now();
        let mut issuer = PrevokeIssuer::new();
        let mut issued_indices = Vec::new();

        // 逐个签发凭证
        for idx in (0..element_slices.len()).step_by(batch_size) {
            let index =
                issuer.add_element(hint::black_box(&elements[idx..s.min(idx + batch_size)]));
            for index in index..s.min(index + batch_size) {
                issued_indices.push(index);
            }
        }

        let dur = start.elapsed();
        results.add_labeled_metric("Acc.Generation", s, dur, Some(s));
        println!(
            "    ✓ {:.2} ms (avg: {:.2} ms per credential)",
            dur.as_secs_f64() * 1000.0,
            dur.as_secs_f64() * 1000.0 / s as f64
        );

        // --- 2. Proof.Generation ---
        println!("  - [Proof.Gen] Generating Witnesses for ALL users...");
        let start = Instant::now();
        // Prevoke 需要为每个用户生成 Merkle Proof
        issuer.generate_proofs_bulk(hint::black_box(s));
        let dur = start.elapsed();
        results.add_labeled_metric("Proof.Generation", s, dur, Some(s));
        println!("    ✓ {:.2} ms", dur.as_secs_f64() * 1000.0);

        // --- 3. Acc.Revoke (10% Individual Revocation) ---
        let removal_10 = (s as f64 * 0.10) as usize;
        let elements_to_revoke = &element_slices[0..removal_10];
        let indices_to_revoke = &issued_indices[0..removal_10];

        println!(
            "  - [Revoke 10%] Revoking {} credentials one by one...",
            removal_10
        );
        let start = Instant::now();
        {
            let mut issuer = issuer.clone();
            // 逐个撤销凭证
            for idx in (0..elements_to_revoke.len()).step_by(batch_size) {
                issuer.revoke_element(
                    hint::black_box(&elements_to_revoke[idx..s.min(idx + batch_size)]),
                    hint::black_box(&indices_to_revoke[idx..s.min(idx + batch_size)]),
                );
            }

            let revoke_dur = start.elapsed();

            // 为剩余有效用户重新生成 Witness
            println!(
                "  - [Proof.Update] Regenerating Witnesses for {} remaining users...",
                s - removal_10
            );
            let start = Instant::now();
            let remaining_count = s - removal_10;
            issuer.generate_proofs_bulk(hint::black_box(remaining_count));

            let proof_update_dur = start.elapsed();
            let total_dur = revoke_dur + proof_update_dur;

            results.add_labeled_metric("Acc.Revoke, 10%", s, total_dur, Some(remaining_count));
            println!(
                "    ✓ Revoke: {:.2} ms, Proof Update: {:.2} ms, Total: {:.2} ms",
                revoke_dur.as_secs_f64() * 1000.0,
                proof_update_dur.as_secs_f64() * 1000.0,
                total_dur.as_secs_f64() * 1000.0
            );
        }

        // --- 4. Acc.Revoke (25% Individual Revocation) ---
        let removal_25 = (s as f64 * 0.25) as usize;
        let elements_to_revoke_25 = &element_slices[0..removal_25];
        let indices_to_revoke_25 = &issued_indices[0..removal_25];

        println!(
            "  - [Revoke 25%] Revoking {} credentials one by one...",
            removal_25
        );
        let start = Instant::now();

        {
            let mut issuer = issuer.clone();
            // 逐个撤销凭证
            for idx in (0..elements_to_revoke_25.len()).step_by(batch_size) {
                issuer.revoke_element(
                    hint::black_box(&elements_to_revoke_25[idx..s.min(idx + batch_size)]),
                    hint::black_box(&indices_to_revoke_25[idx..s.min(idx + batch_size)]),
                );
            }

            let revoke_dur_25 = start.elapsed();

            // 为剩余有效用户重新生成 Witness
            println!(
                "  - [Proof.Update] Regenerating Witnesses for {} remaining users...",
                s - removal_25
            );
            let start = Instant::now();
            let remaining_count_25 = s - removal_25;
            issuer.generate_proofs_bulk(hint::black_box(remaining_count_25));

            let proof_update_dur_25 = start.elapsed();
            let total_dur_25 = revoke_dur_25 + proof_update_dur_25;

            results.add_labeled_metric(
                "Acc.Revoke, 25%",
                s,
                total_dur_25,
                Some(remaining_count_25),
            );
            println!(
                "    ✓ Revoke: {:.2} ms, Proof Update: {:.2} ms, Total: {:.2} ms",
                revoke_dur_25.as_secs_f64() * 1000.0,
                proof_update_dur_25.as_secs_f64() * 1000.0,
                total_dur_25.as_secs_f64() * 1000.0
            );
        }

        // --- 5. Acc.Revoke (50% Individual Revocation) ---
        let removal_50 = (s as f64 * 0.50) as usize;
        let elements_to_revoke_50 = &element_slices[0..removal_50];
        let indices_to_revoke_50 = &issued_indices[0..removal_50];

        println!(
            "  - [Revoke 50%] Revoking {} credentials one by one...",
            removal_50
        );
        let start = Instant::now();
        {
            let mut issuer = issuer.clone();
            // 逐个撤销凭证
            for idx in (0..elements_to_revoke_50.len()).step_by(batch_size) {
                issuer.revoke_element(
                    hint::black_box(&elements_to_revoke_50[idx..s.min(idx + batch_size)]),
                    hint::black_box(&indices_to_revoke_50[idx..s.min(idx + batch_size)]),
                );
            }

            let revoke_dur_50 = start.elapsed();

            // 为剩余有效用户重新生成 Witness
            println!(
                "  - [Proof.Update] Regenerating Witnesses for {} remaining users...",
                s - removal_50
            );
            let start = Instant::now();
            let remaining_count_50 = s - removal_50;
            issuer.generate_proofs_bulk(hint::black_box(remaining_count_50));

            let proof_update_dur_50 = start.elapsed();
            let total_dur_50 = revoke_dur_50 + proof_update_dur_50;

            results.add_labeled_metric(
                "Acc.Revoke, 50%",
                s,
                total_dur_50,
                Some(remaining_count_50),
            );
            println!(
                "    ✓ Revoke: {:.2} ms, Proof Update: {:.2} ms, Total: {:.2} ms",
                revoke_dur_50.as_secs_f64() * 1000.0,
                proof_update_dur_50.as_secs_f64() * 1000.0,
                total_dur_50.as_secs_f64() * 1000.0
            );
        }
    }
}

fn main() {
    let mut results = ExperimentResults::new("prevoke_rs_merkle_benchmark");
    run_prevoke_issuer_benchmarks(&mut results);

    // 保存结果... (复用你的 common 逻辑)
    let _ = results.save_to_file("prevoke_issuer_overheads_results.json");
    print_summary(&results);
}

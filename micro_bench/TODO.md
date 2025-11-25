### **测试计划 TODO List**

#### Phase 1: 核心逻辑实现

- `[x]` **实现 `Issuer` 结构体**:
  - `[x]` 包含一个 `accumulator` 实例 (例如 `SecureMultisetHash`)。
  - `[x]` 实现 `add_elements` 和 `remove_elements` 方法。
  - `[x]` 实现 `generate_proof` 方法，为单个元素生成证明。
  - `[x]` 实现 `generate_proofs_bulk` 方法，用于批量重新生成证明的基准测试。

- `[x]` **实现 `Client` 结构体**:
  - `[x]` 包含凭证/元素本身及其证明。
  - `[x]` 实现 `verify_proof` 方法，用于对照给定的 `accumulator` 摘要进行验证。

- `[x]` **实现 `Blockchain` 模拟**:
  - `[x]` 创建一个简单的结构体，用于存储 `accumulator` 的摘要。
  - `[x]` 集成一个 `RateLimiter` (例如使用 `governor` crate)，模拟 500 TPS 的链上更新限制。

#### Phase 2: 基准测试 (Benchmark) 实现

- `[x]` **配置 `benches/benchmarks.rs`**:
  - `[x]` 设定 `criterion` benchmark groups。

- `[x]` **实现 Issuer 本地计算性能测试**:
  - `[x]` 创建 `issuer_local` benchmark group。
  - `[x]` 循环遍历集合大小 `s` (`1k` 到 `1m`)。
  - `[x]` 为每个 `s` 实现 `add`, `generate_proof`, `remove`, `regenerate_proofs_bulk` 的基准测试。

- `[ ]` **实现 Issuer 端到端性能测试**:
  - `[ ]` 创建 `issuer_e2e` benchmark group。
  - `[ ]` 循环遍历集合大小 `s` (`1k` 到 `50k`)。
  - `[ ]` 实现与本地测试相同的四个场景，但需包含 `Blockchain` 模拟的 `RateLimiter` 延迟。

- `[ ]` **实现 Client 验证性能测试**:
  - `[ ]` 创建 `client_verification` benchmark group。
  - `[ ]` **单个验证**: 针对不同集合大小 `s`，测试单个证明的验证时间。
  - `[ ]` **并发吞吐量**: 设计一个场景（可能需要 `rayon` 或 `tokio`），在单个 benchmark 运行中模拟多个并行验证，并手动计算吞吐量。

#### Phase 3: 批量处理 (Batching) 实现与测试

- `[ ]` **扩展 `Issuer` 以支持 Batching**:
  - `[ ]` 实现 `delta` accumulator 逻辑。
  - `[ ]` 实现基于批量大小 `N` 和时间间隔 `T` 的批次触发机制。
  - `[ ]` 实现 `Acc_new = Acc_old + delta` 的聚合更新逻辑。
  - `[ ]` 实现 `p_new = p_old + delta` 的证明更新逻辑（或提供 `delta` 给客户端）。

- `[ ]` **实现批量处理测试**:
  - `[ ]` 创建 `batching_test` benchmark group。
  - `[ ]` 实现 75% `add` / 25% `remove` 的请求生成器。
  - `[ ]` 循环遍历所有 12 组 `(N, T)` 测试变量组。
  - `[ ]` 在每个测试组中，运行固定时间的模拟（例如 60 秒），并测量**有效吞吐量**和**平均确认延迟**。

#### Phase 4: 结果生成与分析

- `[ ]` **编写结果绘图脚本** (例如 `plot_benchmarks.py`):
  - `[ ]` 脚本需能解析 `criterion` 生成的 benchmark 数据 (通常在 `target/criterion/` 目录下)。
  - `[ ]` 根据“实验结果图表展示”章节的定义，生成所有图表。

- `[ ]` **撰写最终分析报告**:
  - `[ ]` 结合生成的图表，分析各项测试结果。
  - `[ ]` 得出结论，评估不同策略的性能和权衡。


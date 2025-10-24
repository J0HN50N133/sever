# SERVER

## Overview

本项目旨在设计并实现一个基于区块链和增量多重集哈希（Incremental Multiset Hashing, IMH）技术的可撤销可验证凭证（Verifiable Credential, VC）系统。该系统旨在解决传统 VC 撤销机制（如链上撤销列表CRL）所面临的成本高昂、隐私泄露及效率低下的问题。

这个项目是为了这个Idea所设计的一个原型。

## 核心 Idea

1. IMH + 区块链架构: 利用 IMH 作为密码学累加器，将所有有效的凭证聚合为一个恒定大小的根哈希。利用区块链作为去中心化的信任根，仅在链上存储这一个根哈希。这极大降低了链上存储和交易成本，因为无论凭证数量多少，链上状态的大小始终不变。

2. 隐式撤销机制: 凭证的有效性通过其 witness（成员资格证明）与链上最新的根哈希匹配来验证。当发行方（Issuer）从累加器中移除一个凭证并更新链上的根哈希后，该凭证旧的 witness 会自动失效，从而实现高效、匿名的撤销，无需公开“黑名单”。

3. 客户端懒更新策略 (核心优化): 为解决累加器中任何成员变动（增/删）都会导致所有 witness 失效的问题，本方案提出了一种客户端优化策略。
    - 验证者协议不变: 验证者永远只信任并使用链上最新的根哈希进行验证，保证了核心安全性。
    - 客户端智能处理: 用户的客户端（钱包）会首先乐观地使用当前持有的 witness 尝试验证。
    如果验证失败，客户端会自动将其解释为 witness 过期，并在后台向 Issuer 请求一个新的 witness 后重试。
    - 优势: 该策略实现了“懒更新”，用户仅在绝对必要时才更新 witness，在不牺牲安全性的前提下，极大地降低了系统开销和 Issuer 的负载。

4. 批量处理请求: 签名和生成证明请求可以批量处理，我们总是积攒一个BatchSize的请求后再统一处理更新链上的IMH Root(或者`L` ms内的请求，两个阈值谁先达到都可以)，从而进一步提升系统吞吐量。

5. 布隆过滤器：链上添加一个布隆过滤器来快速判断某个凭证是否已经被撤销，从而减少不必要的 witness 请求。

## 实验设计

### 基本假设

1. 所有组件使用gRPC进行通信，网络延迟和带宽足够好，不会成为系统瓶颈。
2. 使用tokio
3. 如果需要存储数据，使用内存存储（HashMap等）来简化实现。

### 系统角色

0. Blockchain（区块链）: 作为去中心化的信任根，存储增量多重集哈希累加器的根哈希值，供验证者查询和验证凭证的有效性。我们通过智能合约来实现这一功能。在这个项目里先用一个简单的接口来模拟区块链的行为。

- 链上存储了IMH的根哈希值（Root Hash）。是一个32字节的字节数组。
- 链上的Accumulaotr有一个版本号，用于标记当前的版本状态。每次更新会使版本号加一。
- 链上有一个智能合约，提供两个主要功能：
  - getAccumulator(): 返回当前的增量多重集哈希累加器的根哈希值。
  - updateAccumulator(newRootHash): 由 Issuer 调用，用于更新根哈希值。
- 这个项目里这个角色就提供rpc接口即可，不需要是真实的区块链。

1. Issuer（发行方）: 负责签发和撤销凭证，维护增量多重集哈希累加器，并将根哈希发布到区块链上。
    - 提供两个主要功能：
      - issue(did): 为指定用户生成一个新的凭证，并将其添加到累加器中，更新根哈希后发布到区块链。
      - revokeCredential(credentialID): 从累加器中移除指定的凭证，更新根哈希后发布到区块链。
      - proof(credentialID): 为指定的凭证生成当前版本对应的 witness，用于用户验证凭证有效性。
    - imh实现使用 <https://github.com/J0HN50N133/minchash>
      主要的数据结构:

```rust
  pub trait MultisetHash: Clone {
    type Proof;
    /// Creates a new, empty multiset hash.
    fn new() -> Self
    where
        Self: Sized;
    /// Adds a single element (provided as a byte slice) to the hash.
    fn add(&mut self, data: &[u8]);
    /// Removes a single element (provided as a byte slice) from the hash.
    fn remove(&mut self, data: &[u8]);
    /// Adds multiple elements (in parallel) to the hash.
    fn add_elements<T>(&mut self, elements: &[T])
    where
        T: AsRef<[u8]> + Sync;
    /// Removes multiple elements (in parallel) from the hash.
    fn remove_elements<T>(&mut self, elements: &[T])
    where
        T: AsRef<[u8]> + Sync;
    /// Returns the current “compressed” state as a vector of bytes.
    fn get_compressed(&self) -> Option<Vec<u8>>;
    /// Returns a 32‑byte digest of the current state.
    fn get_digest(&self) -> Option<Vec<u8>>;

    // generate proof for an element
    fn generate_proof(&self, element: &[u8]) -> Option<Self::Proof>;
    /// verify the element with its proof, mainly called at the user side
    fn verify_proof(&self, element: &[u8], proof: &Self::Proof) -> bool;
}
```

2. platform(平台): 虚拟概念，每个platform有自己的id，签名的时候是按userid+platformid来签名的。用户在不同的平台上使用同一个凭证时，平台可以验证凭证的有效性，但无法关联用户在其他平台上的活动，从而保护用户隐私。
3. 证书: did + witness + version number(和accumulator的版本号一致) + platform id + 签名(issuer签名防篡改)
4. User（用户）: 持有证书，向验证者出示凭证以证明其有效性。

- 在区块链的众包平台上，任意两个用户之间可能会互相联系。需要通过验证对方的凭证有效性。验证流程如下：
    1. 检查凭证的签名是否有效。
    2. 检查凭证的版本号是否与区块链上的累加器版本号一致。
      - 版本号不一致时，向 Issuer 请求新的 witness 并更新凭证，然后重试验证。
    3. 调用proof验证凭证的 witness 是否有效。
    4. 这个校验算法需要实现为一个可以被复用的函数，以便我们进行性能测试。

- 需要预留的参数便于做实验:
  - batch size
  - user数量
  - 平台数量
  - issue/revoke/proof的请求频率

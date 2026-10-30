# Accepted proposal (frozen at grant; transcribed verbatim from the hub on 2026-09-09)

Title: Restore FP8 Delayed-Scaling State Across Distributed Checkpoint Resharding
Category: ML · Subcategory: Distributed Training and Quantization · 4 h expert time · granted 9 Sept 2026 · grant ACTIVE (one immutable ZIP)
Revision f90554efc547ca07331a31fcb4d986cd; proposal id de65d335cff9f690de0b6ff36543a4f9; raw sha256 8c3bc72e…844d

Restore FP8 delayed-scaling state when a distributed training job resumes on a different tensor/pipeline topology. /app/checkpoint.json contains source records with tensor or tie-group, forward/backward role, logical_begin, logical_end, fixed block_size, replica/stage/shard, scale pair, amax ring and next-write cursor. /app/target.json gives new shard intervals and history lengths. A C++20 solver writes /app/restored.json with active states ordered by (tensor,role,stage,shard,global_block), or {"status":"reject"} if consistency fails.

For logical index i, global_block=floor(i/block_size); identity is (tie-group if present else tensor, role, global_block), never physical placement. A shard may cover only part of a block, while several shards or replicas may describe it. Rotate each valid ring once from its next-write cursor into oldest-to-newest order. Records for one block must have equal scale and inverse scale, and overlapping histories must have identical newest suffixes. Choose the longest compatible history, then lowest (replica,stage,shard,tensor). A shorter target keeps newest samples, a longer one left-pads zeros, and output cursor is zero. Incomplete coverage, duplicate targets, invalid cursors, zero history length, scale disagreement or suffix disagreement reject the reconstruction.

This models checkpoint conversion when a large FP8 training run moves between clusters with different TP/PP degrees without resetting calibration and changing later training behavior. Graded layouts combine ragged shard boundaries, partial block coverage, tied tensors, role separation, replicas, topology moves and history resizing. The verifier independently reconstructs blocks from held-out layouts, compares exact canonical state, then replays hidden amax sequences through a frozen integer CPU delayed-scaling model and checks the full next-state trace. No GPU or network is required; candidate labels, exceptions and stdout are not authoritative.

## Reviewer notes (Stage 0)
- Verifiable Strong Accept; Well-specified Accept; Solvable Strong Accept; Difficult Accept; Interesting Strong Accept; Outcome-verified Strong Accept.
- "residual schema and edge-convention details that are not visible in the proposal summary" → every schema and edge convention must be public and exact.
- "the algorithm is specified enough to make the challenge primarily one of exact implementation" → difficulty must come from exact, interacting semantics on compound layouts, not from hidden rules.
- Client-fit: agent-facing text concise, outcome-oriented; procedural statements describe checkpoint semantics, not a mandated workflow; the complete JSON schema must be reproduced in the package.

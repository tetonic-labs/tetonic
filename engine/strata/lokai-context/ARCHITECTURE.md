# Context Compiler Architecture

The `lokai-context` crate is responsible for the progressive assembly, filtering, budgeting, and sealing of contextual evidence for the LLM agents. 
It processes raw data from diverse workspace sources and compiles it into an immutable, deterministically reproducible `ContextPack`.

## Progressive Pipeline (Stages 1-7)

The context compilation follows a strict, 7-stage deterministic pipeline:

1. **Stage 1 (Normalize)**: Evaluates the `ContextRequest` bounds. Ensures paths, budgets, and security tiers are properly constrained before starting retrieval.
2. **Stage 2 (Retrieve)**: Interfaces with the `ContextSourceProvider` to gather initial raw evidence from lexical/semantic searches, LSP symbol resolution, diffs, and project memory.
3. **Stage 3 (Filter)**: Enforces path-based exclusions and data-class ceilings. Drops any evidence that exceeds the session's security classification (e.g. dropping `Confidential` when bounded to `Public`).
4. **Stage 4 (Rank)**: Scores and sorts evidence based on relevance heuristics, prioritizing high-value elements like relevant tests, structural symbols, and direct matches.
5. **Stage 5 (Dedupe)**: Removes exact or near-exact overlapping evidence items (e.g. if semantic search and lexical search yielded the same file block).
6. **Stage 6 (Budget)**: Truncates or drops evidence until the total token size fits within the `TokenBudget` allocated by the request, guaranteeing the final pack will fit into the model's context window.
7. **Stage 7 (Seal)**: Validates workspace fingerprints to ensure consistency. Runs the `SecretScanner` to omit or redact embedded credentials. Computes objective and pack digests. Generates expansion handles, and produces the final `ContextPack`.

## Cache Model

The Context Compiler is heavily reliant on the immutability of sealed context packs. Because each evidence item and the final pack itself carry cryptographic digests (e.g. `ContentDigest` using SHA256), the system guarantees that given the same workspace fingerprint and objective, the compiled result can be cached safely. 

## Agent Integration & Expansion Handles

During stage 7, the compiler generates `ExpansionHandle` references for top-ranked evidence. These handles are securely registered in the `ContextCompiler`'s internal state with short-lived expiration TTLs and execution limits (`max_uses`).

When an agent needs more information about a truncated file or related symbol, it can use the `expand_context(handle_id)` tool. This allows the compiler to lazily fetch additional context (e.g. surrounding lines or connected relationships) without granting the agent arbitrary read access to the entire repository. This enforces a least-privilege, need-to-know model over workspace data.

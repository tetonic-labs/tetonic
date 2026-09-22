# Evaluation Corpus Specification

## Overview
The V2 statistical quality evaluations require a diverse, pinned corpus of repository states to ensure architectural changes do not regress model performance, context accuracy, or patch correctness.

## Corpus Scenarios

### 1. Small Single-Language Repo
- **Characteristics:** < 10 files, standard layout (e.g., simple Rust CLI).
- **Target Metrics:** Minimal overhead, fast Time to First Action.

### 2. Large Monorepo
- **Characteristics:** > 5,000 files, multiple languages, complex dependency graph.
- **Target Metrics:** Context build time, token budget adherence, lack of OOM errors.

### 3. Failing Test Scenario
- **Characteristics:** A workspace with a known syntax error or failing unit test.
- **Target Metrics:** Agent's ability to locate the failure and propose a patch without excessive search tool calls.

### 4. Dirty Worktree
- **Characteristics:** Uncommitted changes that conflict with the agent's goal.
- **Target Metrics:** Safe transaction rollback and explicit user warnings rather than blind overwrites.

### 5. Cross-File Refactoring
- **Characteristics:** An architectural change requiring edits in 3+ dependent files.
- **Target Metrics:** Patch correctness rate (does the resulting code compile and pass tests?).

### 6. Sensitive/Secret Repository
- **Characteristics:** Contains `.env` files, AWS keys, and proprietary algorithms classified as `Secret`.
- **Target Metrics:** Redaction scanner accuracy (0% false negatives for secrets) and enforcement of the `LocalOnly` dispatch rule.

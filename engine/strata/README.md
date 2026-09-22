# Strata Layer (`engine/strata/`)

## Purpose
The **Strata** layer provides institutional memory and knowledge persistence. It manages long-term database state, semantic indexing, abstract syntax tree (AST) code intelligence, dynamic context assembly, and immutable artifact persistence.

## Packages
- [`lokai-memory`](./lokai-memory): Embedded SQLite persistence for sessions, execution histories, and vector embeddings.
- [`lokai-index`](./lokai-index): Codebase AST parsing via tree-sitter, symbol extraction, and semantic search.
- [`lokai-context`](./lokai-context): Intelligent token budget allocation, hierarchical prompt compaction, and context compilation.
- [`lokai-artifact`](./lokai-artifact): Content-addressed artifact store and workspace snapshot management.

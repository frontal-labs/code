# Frontal Code Memory

Semantic memory and lightweight knowledge graph utilities.

## Current Capabilities

- In-memory semantic store with embedding-backed retrieval
- Scoped memory service (`MemoryScope`) for session/repo/branch isolation
- Storage abstractions for metadata/vector/graph responsibilities:
  `MemoryMetadataStore`, `MemoryVectorStore`, and `MemoryGraphStore`
- In-memory backend implementations for those abstractions:
  `InMemoryMetadataStore`, `InMemoryVectorStore`, and `InMemoryGraphStore`
- Persistent file-backed metadata store:
  `PersistentFileMetadataStore`
- Adapter modules for external vector and graph databases:
  `PineconeVectorStoreAdapter` and `Neo4jGraphStoreAdapter`
- Hybrid search (semantic + lexical overlap)
- Configurable minimum-score filtering for empty-result search behavior
- Knowledge graph entities/relations and neighbor queries
- Memory delete semantics across metadata and vector backends
- Embedding model metadata persisted with memory records
- Unit tests for retrieval ranking and graph traversal

## Usage

This crate powers memory tools such as `MemoryUpsert`, `MemorySearch`, and `KnowledgeGraph`.

Runtime backend selection is environment-driven:

- `FRONTAL_CODE_MEMORY_METADATA_PATH` enables `PersistentFileMetadataStore`
- `FRONTAL_CODE_MEMORY_PINECONE_URL` points at the Pinecone index host
- `FRONTAL_CODE_MEMORY_PINECONE_API_KEY` adds Pinecone API-key auth
- `FRONTAL_CODE_MEMORY_PINECONE_NAMESPACE` prefixes per-scope namespaces inside Pinecone
- `FRONTAL_CODE_MEMORY_NEO4J_URL` + `FRONTAL_CODE_MEMORY_NEO4J_DATABASE` enable Neo4j KG storage
- `FRONTAL_CODE_MEMORY_NEO4J_USERNAME` + `FRONTAL_CODE_MEMORY_NEO4J_PASSWORD` add Neo4j basic auth

If those variables are absent, the crate falls back to in-memory metadata/vector/graph stores.

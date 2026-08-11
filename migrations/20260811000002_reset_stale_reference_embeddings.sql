-- Drop reference embeddings produced by the previous embedding model.
--
-- Embeddings were generated locally with fastembed (multilingual-e5-small,
-- 384 dims) before the service moved to the OpenAI embeddings API
-- (text-embedding-3-small, 1536 dims). cosine_similarity returns 0.0 when the
-- two vectors differ in length, so a database still holding 384-dim vectors
-- would score every reference at zero and silently match nothing -- no error,
-- no log, just an engine that never finds a reference.
--
-- Clearing the column is enough: load_cache regenerates and persists any
-- reference whose embedding is NULL on the next startup.

UPDATE clothing_reference SET embedding = NULL;

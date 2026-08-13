# Bundled embedding model

- **Source:** [`minishlab/potion-base-8M`](https://huggingface.co/minishlab/potion-base-8M) (model2vec static embeddings)
- **License:** MIT
- **Fetched once** and vendored so Sara never downloads at runtime.

## Files
- `tokenizer.json` — the model2vec tokenizer (WordPiece; vocab 29528, aligned to the matrix rows).
- `potion-8m-int8.bin` — the embedding matrix, int8-quantized from the original F32 `[29528, 256]`.
- `config.json` — original model config (provenance only; dim=256, normalize=true).

## `potion-8m-int8.bin` format (little-endian)
```
magic  "SEMB"      (4 bytes)
version u32         (=1)
vocab   u32         (=29528)
dim     u32         (=256)
scale   f32         (dequant: f = int8 * scale)
data    int8[vocab*dim]  (row-major; token id -> row)
```

## Inference (model2vec)
tokenize -> gather matrix rows for the token ids -> mean-pool -> L2-normalize.
(PCA + zipf weighting are already baked into the stored matrix.)

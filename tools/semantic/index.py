"""Строит локальный семантический индекс по crates/ через LM Studio.

Запуск: pwsh -File tools/semantic-search.ps1 -Rebuild
Индекс пишется в каталог из FLINT_SEMANTIC_INDEX (по умолчанию .tmp\\semantic),
потому что tools/ может быть доступен только на чтение.
"""
import json
import os
import sys
import time

import numpy as np

import common

CHUNK_LINES = 32
OVERLAP = 8
SRC = os.path.join(common.ROOT, "crates")


def chunks_for(path):
    with open(path, encoding="utf-8", errors="replace") as fh:
        lines = fh.read().splitlines()
    if not lines:
        return []
    step = CHUNK_LINES - OVERLAP
    out = []
    for start in range(0, len(lines), step):
        block = lines[start:start + CHUNK_LINES]
        if not block:
            break
        text = "\n".join(block).strip()
        if len(text) >= 40:
            out.append((start + 1, start + len(block), text))
        if start + CHUNK_LINES >= len(lines):
            break
    return out


def main():
    if not os.path.isdir(SRC):
        print(f"нет каталога {SRC}", file=sys.stderr)
        return 1

    files = []
    for base, dirs, names in os.walk(SRC):
        dirs[:] = [d for d in dirs if d not in ("target", ".git")]
        files.extend(os.path.join(base, n) for n in names if n.endswith(".rs"))
    files.sort()

    meta = []
    for path in files:
        rel = os.path.relpath(path, common.ROOT)
        for start, end, text in chunks_for(path):
            meta.append({"path": rel, "start": start, "end": end, "text": text})

    print(f"files={len(files)} chunks={len(meta)} model={common.EMBED_MODEL} index={common.INDEX_DIR}", flush=True)
    vecs = []
    batch = 16
    t0 = time.time()
    try:
        for i in range(0, len(meta), batch):
            part = meta[i:i + batch]
            vecs.extend(common.embed_batch([common.DOC_PREFIX + m["text"] for m in part]))
            if (i // batch) % 20 == 0:
                print(f"{i + len(part)}/{len(meta)} {time.time() - t0:.1f}s", flush=True)
    except (OSError, RuntimeError) as exc:
        print(f"эмбеддер недоступен ({common.EMBED_MODEL} на {common.BASE}): {exc}", file=sys.stderr)
        return 1

    matrix = np.asarray(vecs, dtype=np.float32)
    os.makedirs(common.INDEX_DIR, exist_ok=True)
    np.savez_compressed(os.path.join(common.INDEX_DIR, "index.npz"), vecs=matrix)
    with open(os.path.join(common.INDEX_DIR, "meta.json"), "w", encoding="utf-8") as fh:
        json.dump(meta, fh, ensure_ascii=False)
    print(f"done chunks={len(meta)} dim={matrix.shape[1]} in {time.time() - t0:.1f}s")
    return 0


if __name__ == "__main__":
    sys.exit(main())

"""Семантический поиск по crates/ через локальный индекс.

  pwsh -File tools/semantic-search.ps1 "timelock expired before refund"
  pwsh -File tools/semantic-search.ps1 -Ru "где проверяется, что таймлок истёк"

Русский запрос (-Ru) переводится локальной моделью в английские термины: это нужно
для английских эмбеддеров вроде nomic-embed. Для многоязычного bge-m3 перевод
не требуется - он и так ставит русский и английский в одно пространство.
"""
import json
import os
import sys

import numpy as np

import common

GLOSSARY = (
    "таймлок=timelock, возврат=refund, своп=swap, оффер=offer, кошелёк=wallet, "
    "нода=node, монеты=coins, высота блока=block height, seed-фраза=seed phrase"
)


def translate(query):
    prompt = (
        "Переведи запрос на английский для поиска по Rust-коду. Используй термины: "
        + GLOSSARY
        + ". Верни только перевод одной строкой, без кавычек.\nЗапрос: "
        + query
    )
    data = common.post_json(
        common.BASE + "/v1/chat/completions",
        {"model": common.CHAT_MODEL, "messages": [{"role": "user", "content": prompt}],
         "max_tokens": 60, "temperature": 0},
    )
    return data["choices"][0]["message"]["content"].strip().strip('"')


def main():
    args = sys.argv[1:]
    if not args:
        print(__doc__)
        return 2
    use_ru = args[0] == "--ru"
    if use_ru:
        args = args[1:]
    if not args:
        print(__doc__)
        return 2

    query = args[0]
    top_k = int(args[1]) if len(args) > 1 else 8

    index_path = os.path.join(common.INDEX_DIR, "index.npz")
    if not os.path.exists(index_path):
        print(f"нет индекса в {common.INDEX_DIR}: запусти tools/semantic-search.ps1 -Rebuild", file=sys.stderr)
        return 1

    if use_ru:
        try:
            query = translate(query)
            print(f"query(en): {query}")
        except Exception as exc:  # модель перевода не загружена - ищем как есть
            print(f"translate failed ({exc}), query as-is", file=sys.stderr)

    vecs = np.load(index_path)["vecs"]
    with open(os.path.join(common.INDEX_DIR, "meta.json"), encoding="utf-8") as fh:
        meta = json.load(fh)

    try:
        q = np.asarray(common.embed_one(query), dtype=np.float32)
    except (OSError, RuntimeError) as exc:
        print(f"эмбеддер недоступен ({common.EMBED_MODEL} на {common.BASE}): {exc}", file=sys.stderr)
        return 1
    q /= np.linalg.norm(q) + 1e-9
    scores = (vecs @ q) / (np.linalg.norm(vecs, axis=1) + 1e-9)

    for rank, idx in enumerate(np.argsort(-scores)[:top_k], 1):
        m = meta[int(idx)]
        head = m["text"].strip().splitlines()[0][:110]
        print(f"{rank:2d}. {scores[idx]:.3f}  {m['path']}:{m['start']}-{m['end']}")
        print(f"    {head}")
    return 0


if __name__ == "__main__":
    sys.exit(main())

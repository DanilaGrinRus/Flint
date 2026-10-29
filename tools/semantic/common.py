"""Общие настройки эмбеддера и HTTP для semantic-поиска.

Модель, эндпоинт и каталог индекса берутся из окружения:
LMSTUDIO_EMBED_MODEL, LMSTUDIO_CHAT_MODEL, LMSTUDIO_URL, FLINT_SEMANTIC_INDEX.
"""
import json
import os
import urllib.error
import urllib.request

EMBED_MODEL = os.environ.get("LMSTUDIO_EMBED_MODEL", "text-embedding-bge-m3")
CHAT_MODEL = os.environ.get("LMSTUDIO_CHAT_MODEL", "qwen2.5-coder-14b-instruct")
BASE = os.environ.get("LMSTUDIO_URL", "http://127.0.0.1:1234").rstrip("/")
HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(HERE))
INDEX_DIR = os.environ.get("FLINT_SEMANTIC_INDEX") or os.path.join(ROOT, ".tmp", "semantic")

# Префиксы задач нужны nomic-embed-text; bge-m3 и nomic-embed-code их не ждут.
# Переопределяются через FLINT_DOC_PREFIX / FLINT_QUERY_PREFIX (пустая строка - без префикса).
if os.environ.get("FLINT_DOC_PREFIX") is not None:
    DOC_PREFIX = os.environ["FLINT_DOC_PREFIX"]
    QUERY_PREFIX = os.environ.get("FLINT_QUERY_PREFIX", "")
elif "nomic-embed-text" in EMBED_MODEL.lower():
    DOC_PREFIX, QUERY_PREFIX = "search_document: ", "search_query: "
else:
    DOC_PREFIX, QUERY_PREFIX = "", ""


def post_json(url, payload, timeout=180):
    body = json.dumps(payload).encode("utf-8")
    req = urllib.request.Request(url, data=body, headers={"Content-Type": "application/json"})
    try:
        with urllib.request.urlopen(req, timeout=timeout) as resp:
            return json.loads(resp.read().decode("utf-8"))
    except urllib.error.HTTPError as exc:
        detail = exc.read().decode("utf-8", "replace")[:300]
        raise RuntimeError(f"LM Studio {exc.code}: {detail}") from None


def embed_batch(texts):
    data = post_json(BASE + "/v1/embeddings", {"model": EMBED_MODEL, "input": texts})
    return [item["embedding"] for item in data["data"]]


def embed_one(text):
    return embed_batch([QUERY_PREFIX + text])[0]

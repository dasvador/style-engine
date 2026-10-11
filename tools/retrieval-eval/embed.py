"""레퍼런스 검색 평가용 벡터 스냅샷을 만든다.

CI 는 임베딩 API 를 부르지 않는다. 대신 이 스크립트가 만든
`tests/eval/retrieval_vectors.json` 을 커밋하고, `tests/retrieval_eval.rs` 가 그것으로
순위를 잰다.

스냅샷은 벡터와 함께 **임베딩한 원문**을 담는다. 테스트는 카탈로그의 현재 검색 문장과
그 원문을 비교해서, 다르면 실패한다 — 검색 문장을 고치고 스냅샷을 다시 만들지 않으면
옛 벡터로 잰 숫자가 나오는데, 그 숫자는 틀렸다는 표시가 어디에도 없기 때문이다.

다시 만들 때는 원문이 바뀐 것만 임베딩한다.

    python3 tools/retrieval-eval/embed.py

`.env` 의 `OPENAI_API_KEY` 를 쓴다. 모델은 서비스 기본값과 같은 text-embedding-3-small.
"""
import base64
import json
import pathlib
import struct
import tomllib
import urllib.request

ROOT = pathlib.Path(__file__).resolve().parents[2]
CATALOG = ROOT / "data" / "clothing_references.toml"
CASES = ROOT / "tests" / "fixtures" / "retrieval_cases.toml"
OUT = ROOT / "tests" / "eval" / "retrieval_vectors.json"
MODEL = "text-embedding-3-small"


def api_key() -> str:
    for line in (ROOT / ".env").read_text().splitlines():
        if line.startswith("OPENAI_API_KEY="):
            return line.split("=", 1)[1].strip().strip('"').strip("'")
    raise SystemExit(".env 에 OPENAI_API_KEY 가 없다")


def search_text(ref: dict) -> str:
    # `ClothingReference::text_for_embedding` 과 같은 규칙.
    t = (ref.get("embedding_text") or "").strip()
    return t if t else ref["description"].strip()


def encode(vec: list[float]) -> str:
    return base64.b64encode(struct.pack(f"<{len(vec)}f", *vec)).decode()


def embed(texts: list[str]) -> list[list[float]]:
    req = urllib.request.Request(
        "https://api.openai.com/v1/embeddings",
        data=json.dumps({"model": MODEL, "input": texts}).encode(),
        headers={"Authorization": "Bearer " + api_key(), "Content-Type": "application/json"},
    )
    data = json.loads(urllib.request.urlopen(req).read())["data"]
    return [d["embedding"] for d in sorted(data, key=lambda d: d["index"])]


def main() -> None:
    refs = tomllib.loads(CATALOG.read_text(encoding="utf-8"))["references"]
    cases = tomllib.loads(CASES.read_text(encoding="utf-8"))["cases"]
    want = {("references", r["name"]): search_text(r) for r in refs}
    want.update({("queries", c["id"]): c["query"].strip() for c in cases})

    old = json.loads(OUT.read_text(encoding="utf-8")) if OUT.exists() else {}
    if old.get("model") != MODEL:
        old = {}
    out = {"model": MODEL, "references": {}, "queries": {}}
    todo = []
    for (kind, key), text in want.items():
        prev = old.get(kind, {}).get(key)
        if prev and prev["text"] == text:
            out[kind][key] = prev
        else:
            todo.append((kind, key, text))

    for i in range(0, len(todo), 64):
        chunk = todo[i : i + 64]
        for (kind, key, text), vec in zip(chunk, embed([t for _, _, t in chunk])):
            out[kind][key] = {"text": text, "vector": encode(vec)}

    for kind in ("references", "queries"):
        out[kind] = dict(sorted(out[kind].items()))
    OUT.write_text(json.dumps(out, ensure_ascii=False, indent=1) + "\n", encoding="utf-8")
    print(f"임베딩 {len(todo)}건 새로 생성, 레퍼런스 {len(out['references'])} / 질의 {len(out['queries'])} → {OUT.relative_to(ROOT)}")


if __name__ == "__main__":
    main()

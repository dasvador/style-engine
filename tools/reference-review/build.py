"""레퍼런스 검수 페이지를 만든다.

카탈로그(`data/clothing_references.toml`)에서 검수할 항목을 골라 `template.html`
에 넣고 한 장짜리 HTML 을 쓴다. 고르는 항목은 두 가지다.

- `review_status = "draft"` 인 레퍼런스 — 승인 / 수정 필요 / 제외
- `# 검수 대기 — 제안한 검색 문장(적용 안 됨):` 주석이 붙은 레퍼런스 — 적용 / 적용 안 함

쓰는 법:
    python3 tools/reference-review/build.py /tmp/reference-review.html

만든 파일은 Claude 가 db 기능이 있는 아티팩트로 게시하고, 결정은 그 저장소의
`decisions` 컬렉션에 쌓인다. 각 문서는 `{name, kind, decision, note}` 다.

문서 id 는 레퍼런스 이름의 해시라서, 같은 아티팩트에 다시 게시해도 이전 회차의
결정이 다른 항목에 붙지 않는다.
"""
import hashlib
import json
import pathlib
import re
import sys
import tomllib

ROOT = pathlib.Path(__file__).resolve().parents[2]
CATALOG = ROOT / "data" / "clothing_references.toml"
TEMPLATE = pathlib.Path(__file__).with_name("template.html")
PROPOSAL = re.compile(r"# 검수 대기 — 제안한 검색 문장\(적용 안 됨\):\n# (.+)(?:\n# 근거: (.+))?")
GENRE_LABEL = {
    "minimal": "미니멀", "classic": "클래식", "romantic": "로맨틱",
    "modern_chic": "모던 시크", "bohemian": "보헤미안", "street": "스트리트",
    "mannish": "매니시", "sporty_casual": "스포티 캐주얼", "amekaji": "아메카지",
    "preppy": "프레피", "workwear": "워크웨어", "outdoor_casual": "아웃도어 캐주얼",
}


def key(kind: str, name: str) -> str:
    return f"{kind[0]}-{hashlib.sha1(name.encode()).hexdigest()[:12]}"


def main(out: str) -> None:
    raw = CATALOG.read_text(encoding="utf-8")
    refs = tomllib.loads(raw)["references"]
    proposals = {}
    for block in raw.split("[[references]]")[1:]:
        nm = re.search(r'^name = "([^"]+)"', block, re.M)
        pr = PROPOSAL.search(block)
        if nm and pr:
            proposals[nm.group(1)] = (pr.group(1).strip(), (pr.group(2) or "").strip())

    items = []
    for r in refs:
        if r["review_status"] == "draft":
            kind = "draft"
        elif r["name"] in proposals:
            kind = "seed"
        else:
            continue
        item = {
            "key": key(kind, r["name"]),
            "kind": kind,
            "name": r["name"],
            "category": r["category"],
            "subcategory": r.get("subcategory"),
            "era": r.get("era"),
            "genres": r.get("genres", []),
            "source_note": r.get("source_note", ""),
            "flag": "검수 필요" in r.get("source_note", ""),
            "embedding_text": (r.get("embedding_text") or "").strip(),
            "description": r["description"].strip(),
        }
        if kind == "seed":
            item["proposal"], item["evidence"] = proposals[r["name"]]
        items.append(item)

    if not items:
        sys.exit("검수할 항목이 없다 (draft 도, 제안 검색 문장도 없음)")

    data = json.dumps({"items": items, "genreLabel": GENRE_LABEL}, ensure_ascii=False)
    html = TEMPLATE.read_text(encoding="utf-8").replace("__DATA__", data.replace("</", "<\\/"))
    pathlib.Path(out).write_text(html, encoding="utf-8")
    drafts = sum(i["kind"] == "draft" for i in items)
    print(f"신규 {drafts}건, 검색 문장 제안 {len(items) - drafts}건 → {out}")


if __name__ == "__main__":
    main(sys.argv[1] if len(sys.argv) > 1 else "reference-review.html")

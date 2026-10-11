"""옷장 속성 검수 페이지를 만든다.

검수 파일 하나를 `template.html` 에 넣어 한 장짜리 HTML 을 쓴다. 파일에 들어갈 수
있는 항목은 세 가지다.

- `[[assign]]` 텍스처 월드 배정 (cases.toml)
- `[[combo]]`  조합 판단 — 충돌/보완/상관없음 (cases.toml). 각 옷의 지금 월드를
  보여 주려고 로컬 DB 를 한 번 읽는다(`.env` 의 DATABASE_URL, mysql 클라이언트 필요).
- `[[role]]`   아우터 역할 (outer_roles.toml). 역할별 점수 영향(`impact`)을 함께 보여 준다.

    python3 tools/world-review/build.py /tmp/review.html [검수 파일, 기본 cases.toml]

Claude 가 db 기능이 있는 아티팩트로 게시하고, 결정은 `decisions` 컬렉션에
`{kind, name, decision, note}` 로 쌓인다. 문서 id 는 회차와 이름의 해시다.
"""
import hashlib
import json
import pathlib
import re
import shutil
import subprocess
import sys
import tomllib
import urllib.parse

ROOT = pathlib.Path(__file__).resolve().parents[2]
CASES = pathlib.Path(__file__).with_name("cases.toml")
TEMPLATE = pathlib.Path(__file__).with_name("template.html")


def key(kind: str, round_: str, name: str) -> str:
    return f"{kind[0]}-{hashlib.sha1(f'{round_}:{name}'.encode()).hexdigest()[:12]}"


def current_worlds() -> dict[str, list[str]]:
    env = (ROOT / ".env").read_text(encoding="utf-8")
    url = re.search(r"^DATABASE_URL=(.+)$", env, re.M).group(1).strip()
    m = re.match(r"mysql://([^:]+):([^@]*)@([^:/]+)(?::(\d+))?/(.+)", url)
    user, pw, host, port, db = m.groups()
    sql = (
        "SELECT c.name, IFNULL(GROUP_CONCAT(t.texture_world ORDER BY t.texture_world), '') "
        "FROM clothing c LEFT JOIN clothing_texture_world t ON t.clothing_id = c.id "
        "WHERE c.gender = 'female' GROUP BY c.id"
    )
    out = subprocess.run(
        [shutil.which("mysql"), "--default-character-set=utf8mb4", "-h", host, "-P", port or "3306",
         "-u", user, db, "-N", "-e", sql],
        env={"MYSQL_PWD": urllib.parse.unquote(pw)}, capture_output=True, text=True, check=True,
    ).stdout
    worlds = {}
    for line in out.splitlines():
        name, w = (line.split("\t") + [""])[:2]
        worlds[name] = [x for x in w.split(",") if x]
    return worlds


def main(out: str, cases: pathlib.Path = CASES) -> None:
    data = tomllib.loads(cases.read_text(encoding="utf-8"))
    round_ = data["round"]
    worlds = current_worlds() if data.get("combo") else {}
    proposed = {a["name"]: a["proposal"] for a in data.get("assign", [])}

    items = []
    for r in data.get("role", []):
        items.append({"key": key("role", round_, r["name"]), "kind": "role", "name": r["name"],
                      "proposal": r["proposal"], "why": r.get("why", ""), "impact": r.get("impact", {})})
    for a in data.get("assign", []):
        items.append({"key": key("assign", round_, a["name"]), "kind": "assign", "name": a["name"],
                      "proposal": a["proposal"], "why": a.get("why", "")})
    for c in data.get("combo", []):
        pieces = []
        for n in c["items"]:
            if n not in worlds:
                sys.exit(f"옷장에 없는 이름: {n}")
            pieces.append({"name": n, "worlds": worlds[n], "proposed": proposed.get(n)})
        items.append({"key": key("combo", round_, c["id"]), "kind": "combo", "name": c["id"],
                      "pair": c["pair"], "pieces": pieces})

    payload = json.dumps({"round": round_, "items": items}, ensure_ascii=False)
    html = TEMPLATE.read_text(encoding="utf-8").replace("__DATA__", payload.replace("</", "<\\/"))
    pathlib.Path(out).write_text(html, encoding="utf-8")
    n = {k: sum(i["kind"] == k for i in items) for k in ("role", "assign", "combo")}
    print(f"아우터 역할 {n['role']}건, 월드 배정 {n['assign']}건, 조합 판단 {n['combo']}건 → {out}")


if __name__ == "__main__":
    main(sys.argv[1] if len(sys.argv) > 1 else "world-review.html",
         pathlib.Path(sys.argv[2]) if len(sys.argv) > 2 else CASES)

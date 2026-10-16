"""사용 반응으로 쌓인 비교 쌍(`look_pair` 뷰)을 테스트 픽스처로 내보낸다.

    python3 tools/look-pairs/export.py

쓰는 파일:
  tests/fixtures/usage_pairs.toml       — 쌍 목록. tests/eval_scorecard.rs 가 읽는다.
  tests/fixtures/wardrobe_registry.toml — 맨 끝 '반응 쌍용 아이템' 구역만 다시 쓴다.

여러 번 돌려도 결과가 같다. 쌍 id 는 (요청, 이긴 룩, 진 룩) 으로 정해지고, 아이템
속성은 실행 시점의 DB 값 그대로 적는다. 이미 등록된 아이템과 이름·속성이 모두 같으면
그 키를 다시 쓰고, 하나라도 다르면 새 키를 만든다 — 기존 케이스의 값을 건드리지 않는다.

DB 접속 정보는 .env 의 DATABASE_URL 에서 읽는다. 결과 파일에는 들어가지 않는다.
"""

import hashlib
import json
import os
import pathlib
import re
import shutil
import subprocess
import urllib.parse

ROOT = pathlib.Path(__file__).resolve().parents[2]
REGISTRY = ROOT / "tests/fixtures/wardrobe_registry.toml"
PAIRS = ROOT / "tests/fixtures/usage_pairs.toml"
MARK = "# ─── 반응 쌍용 아이템 (tools/look-pairs/export.py 가 다시 쓴다) ───"

SLOTS = {"상의": "top", "하의": "bottom", "아우터": "outer", "신발": "shoes", "가방": "bag"}
SLOT_ORDER = ["top", "bottom", "outer", "shoes", "bag"]
MOODS_KO = {"female": "여성", "male": "남성"}


def query(sql: str) -> list[dict]:
    line = next(l for l in (ROOT / ".env").read_text().splitlines() if l.startswith("DATABASE_URL"))
    u = urllib.parse.urlparse(line.split("=", 1)[1].strip().strip('"'))
    mysql = shutil.which("mysql") or "/opt/homebrew/bin/mysql"
    env = dict(os.environ, MYSQL_PWD=urllib.parse.unquote(u.password or ""))
    out = subprocess.run(
        [mysql, "--default-character-set=utf8mb4", "--batch", "--raw", "-u", u.username,
         "-h", u.hostname, "-P", str(u.port or 3306), u.path[1:], "-e", sql],
        env=env, capture_output=True, text=True, check=True,
    ).stdout.splitlines()
    if not out:
        return []
    head = out[0].split("\t")
    return [dict(zip(head, (None if v == "NULL" else v for v in r.split("\t")))) for r in out[1:]]


def toml_str(s: str) -> str:
    return json.dumps(s, ensure_ascii=False)


def item_block(row: dict, seasons: list[str], worlds: list[str]) -> list[str]:
    """레지스트리 한 항목의 본문(머리 줄 제외). 비교에도 이 문자열을 쓴다."""
    lines = [f"name = {toml_str(row['name'])}", f"category = {toml_str(row['category'])}"]
    for col, key in [
        ("thickness", "thickness"), ("style", "style"), ("role", "role"), ("tone", "tone"),
        ("saturation", "saturation"), ("color_temperature", "color_temperature"),
        ("weight", "weight"),
    ]:
        if row[col]:
            lines.append(f"{key} = {toml_str(row[col])}")
    if row["formality_level"]:
        lines.append(f"formality_level = {int(row['formality_level'])}")
    if row["silhouette_volume"]:
        lines.append(f"silhouette = {toml_str(row['silhouette_volume'])}")
    if seasons:
        lines.append("seasons = [" + ", ".join(toml_str(s) for s in sorted(seasons)) + "]")
    if worlds:
        lines.append("texture_worlds = [" + ", ".join(toml_str(w) for w in sorted(worlds)) + "]")
    return lines


def existing_items(text: str) -> dict[str, str]:
    """구역 밖 레지스트리 항목: 본문 → 키."""
    found = {}
    for m in re.finditer(r"^\[items\.([^\]]+)\]\n((?:[^\[\n#][^\n]*\n)+)", text, re.M):
        found[m.group(2).strip()] = m.group(1)
    return found


def main() -> None:
    pairs = query(
        "SELECT request_id, gender, mood_key, better_look_id, worse_look_id, strength, "
        "better_position, worse_position, better_outfit, worse_outfit, created_at "
        "FROM look_pair ORDER BY created_at, request_id, better_look_id, worse_look_id"
    )
    ids = sorted({o["id"] for p in pairs for k in ("better_outfit", "worse_outfit")
                  for o in json.loads(p[k])})
    in_list = ",".join(f"'{i}'" for i in ids)
    rows = {r["id"]: r for r in query(
        "SELECT id, name, category, thickness, style, role, tone, saturation, color_temperature, "
        f"weight, formality_level, silhouette_volume FROM clothing WHERE id IN ({in_list})")}
    seasons: dict[str, list[str]] = {}
    for r in query(f"SELECT clothing_id, season FROM clothing_season WHERE clothing_id IN ({in_list})"):
        seasons.setdefault(r["clothing_id"], []).append(r["season"])
    worlds: dict[str, list[str]] = {}
    for r in query("SELECT clothing_id, texture_world FROM clothing_texture_world "
                   f"WHERE clothing_id IN ({in_list})"):
        worlds.setdefault(r["clothing_id"], []).append(r["texture_world"])

    text = REGISTRY.read_text()
    base = text.split(MARK)[0].rstrip() + "\n"
    known = existing_items(base)

    keys: dict[str, str] = {}
    new_blocks: list[str] = []
    for cid in ids:
        body = "\n".join(item_block(rows[cid], seasons.get(cid, []), worlds.get(cid, [])))
        if body in known:
            keys[cid] = known[body]
            continue
        # 키는 DB id 에서 만든다 — 쌍이 늘어도 이미 나간 키가 바뀌지 않는다.
        key = "lp_" + hashlib.sha1(cid.encode()).hexdigest()[:6]
        keys[cid] = key
        new_blocks.append(f"[items.{key}]\n{body}\n")

    REGISTRY.write_text(
        base + "\n" + MARK + "\n# 값은 내보낸 시점의 DB 그대로다.\n\n" + "\n".join(new_blocks)
    )

    out = [
        "# 홈 추천 한 번에 나온 코디들 중, 사용자가 남긴 반응으로 더 나은 쪽이 정해진 쌍.",
        "# tools/look-pairs/export.py 가 DB 의 `look_pair` 뷰에서 만든다 — 손으로 고치지 않는다.",
        "#",
        "# better 는 좋아요/입어봄을 받고 지우지 않은 코디, worse 는 같은 요청에서 지운 코디(strong)",
        "# 이거나 아무 반응도 없던 코디(weak)다. `preference_pairs.toml` 의 블라인드 비교와 달리",
        "# 운영 엔진이 고른 후보끼리의 비교라, 운영 엔진이 이미 걸러 낸 뒤의 취향을 잰다.",
        "version = 1",
        "",
    ]
    for p in pairs:
        pid = hashlib.sha1(
            f"{p['request_id']}:{p['better_look_id']}:{p['worse_look_id']}".encode()
        ).hexdigest()[:8]
        def outfit(raw: str) -> str:
            slots = {SLOTS[o["category"]]: keys[o["id"]] for o in json.loads(raw)}
            return "{ " + ", ".join(f'{s} = "{slots[s]}"' for s in SLOT_ORDER if s in slots) + " }"
        out += [
            "[[pairs]]",
            f'id = "LP{pid}"',
            f"context = {toml_str(MOODS_KO.get(p['gender'] or '', '?') + ' · ' + (p['mood_key'] or '?'))}",
            'situation = "일상"',
            f'strength = "{p["strength"]}"',
            f"positions = [{p['better_position']}, {p['worse_position']}]",
            f"better = {outfit(p['better_outfit'])}",
            f"worse = {outfit(p['worse_outfit'])}",
            "",
        ]
    PAIRS.write_text("\n".join(out))
    print(f"{len(pairs)}쌍, 새 아이템 {len(new_blocks)}개, 기존 키 재사용 {len(ids) - len(new_blocks)}개")


if __name__ == "__main__":
    main()

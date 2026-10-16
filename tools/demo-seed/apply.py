"""데모 시드 SQL 하나를 .env 의 DATABASE_URL 이 가리키는 DB 에 넣는다.

    python3 tools/demo-seed/apply.py seeds/demo/glam.sql

시드는 마이그레이션이 아니라서 서버가 뜰 때 자동으로 들어가지 않는다. 개발·데모 DB 에만
손으로 넣는다. 접속 정보는 출력하지 않는다. 대상 호스트가 localhost 가 아니면 묻는다.
"""

import os
import pathlib
import shutil
import subprocess
import sys
import urllib.parse

ROOT = pathlib.Path(__file__).resolve().parents[2]


def main() -> None:
    if len(sys.argv) != 2:
        sys.exit("사용법: python3 tools/demo-seed/apply.py <seed.sql>")
    sql = pathlib.Path(sys.argv[1])
    line = next(l for l in (ROOT / ".env").read_text().splitlines() if l.startswith("DATABASE_URL"))
    u = urllib.parse.urlparse(line.split("=", 1)[1].strip().strip('"'))
    if u.hostname not in ("localhost", "127.0.0.1", "::1"):
        answer = input(f"{u.hostname} 은 로컬 DB 가 아닙니다. 그래도 넣을까요? [y/N] ")
        if answer.strip().lower() != "y":
            sys.exit("중단했습니다.")
    mysql = shutil.which("mysql") or "/opt/homebrew/bin/mysql"
    env = dict(os.environ, MYSQL_PWD=urllib.parse.unquote(u.password or ""))
    with sql.open("rb") as f:
        subprocess.run(
            [mysql, "--default-character-set=utf8mb4", "-u", u.username, "-h", u.hostname,
             "-P", str(u.port or 3306), u.path[1:]],
            stdin=f, env=env, check=True,
        )
    print(f"{sql} 적용 완료 ({u.hostname}{u.path})")


if __name__ == "__main__":
    main()

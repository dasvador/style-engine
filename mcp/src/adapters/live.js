// 실서비스 읽기 어댑터 — 연결된 사용자의 프로필과 **본인** 옷장만 읽는다(live 모드 전용).
//
// 규칙:
//   - 사용자 id 는 언제나 인증 주체(OAuth 토큰에 묶인 id)에서 온다. 도구 인자로 받지 않는다.
//   - 모든 옷장 질의는 `clothing.user_id = ?` 로 거른다(Rust 마이그레이션 20261024000001 이 만든 컬럼).
//     컬럼이 없으면(마이그레이션 전) 시작을 거부한다 — 전역 옷장을 읽는 일이 없게.
//   - 읽기만 한다(SELECT). 등록·수정·삭제·이미지 생성 경로는 없다. 실제 추천은 여기서 하지 않고 Rust 에 맡긴다(recommend.js).
//   - 옷 사진(image_url, 대개 data URI)은 내보내지 않는다. 있는지만 알린다.
// 접속 문자열(LIVE_DATABASE_URL)은 로그에 남기지 않는다.

import mysql from 'mysql2/promise';

const CATEGORIES = ['아우터', '상의', '하의', '신발', '가방'];
const MAX_ITEMS = 200;

export class LiveAdapter {
  #pool;

  /** @param {import('mysql2/promise').Pool} pool */
  constructor(pool) {
    this.#pool = pool;
  }

  static async connect(databaseUrl) {
    const pool = mysql.createPool({ uri: databaseUrl, connectionLimit: 4, dateStrings: true });
    const adapter = new LiveAdapter(pool);
    await adapter.checkSchema();
    return adapter;
  }

  async checkSchema() {
    const [rows] = await this.#pool.query(
      "SELECT COUNT(*) AS n FROM information_schema.COLUMNS WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = 'clothing' AND COLUMN_NAME = 'user_id'",
    );
    if (Number(rows[0].n) !== 1) {
      throw new Error('clothing.user_id 가 없어요. Rust 마이그레이션(20261024000001)을 먼저 적용해야 live 모드를 켤 수 있어요.');
    }
  }

  async exists(userId) {
    if (typeof userId !== 'string' || userId.length === 0 || userId.length > 50) return false;
    const [rows] = await this.#pool.execute('SELECT 1 FROM app_user WHERE id = ? LIMIT 1', [userId]);
    return rows.length === 1;
  }

  /** 내 프로필. 토큰·내부 값은 싣지 않는다. */
  async getProfile(userId) {
    const [users] = await this.#pool.execute('SELECT username, display_name FROM app_user WHERE id = ?', [userId]);
    if (users.length === 0) return null;
    const [counts] = await this.#pool.execute(
      'SELECT category, COUNT(*) AS n FROM clothing WHERE user_id = ? GROUP BY category',
      [userId],
    );
    return {
      username: users[0].username,
      display_name: users[0].display_name ?? null,
      wardrobe_count: counts.reduce((sum, r) => sum + Number(r.n), 0),
      wardrobe_by_category: Object.fromEntries(counts.map((r) => [r.category, Number(r.n)])),
    };
  }

  /** 내 옷장. category 는 표준 다섯 개 중 하나만. */
  async listWardrobe(userId, { category = null, limit = 50 } = {}) {
    if (category !== null && !CATEGORIES.includes(category)) throw new Error('알 수 없는 카테고리');
    const n = Math.min(Math.max(1, Number(limit) || 50), MAX_ITEMS);
    const params = [userId];
    let sql =
      'SELECT id, name, category, color, sub_category, material_primary, (image_url IS NOT NULL AND image_url <> \'\') AS has_image ' +
      'FROM clothing WHERE user_id = ?';
    if (category) {
      sql += ' AND category = ?';
      params.push(category);
    }
    sql += ` ORDER BY created_at DESC, id LIMIT ${n}`;
    const [rows] = await this.#pool.execute(sql, params);
    return rows.map((r) => ({
      id: r.id,
      name: r.name,
      category: r.category,
      color: r.color ?? null,
      sub_category: r.sub_category ?? null,
      material: r.material_primary ?? null,
      has_image: Boolean(Number(r.has_image)),
    }));
  }

  /**
   * 추천 전에 확인할 것(모델을 부르기 전에 막을 수 있는 것): 옷장 수(성별을 고르면 그 성별), 지역 이름,
   * 고른 무드가 그 성별에 있는지. 값은 사용자 본인 것만 읽는다.
   * @returns {Promise<{ wardrobe: number, region: string|null, moodOk: boolean }>}
   */
  async recommendContext(userId, { gender = null, mood = null } = {}) {
    // Rust list_clothing_filtered 와 같은 조건(gender = ?).
    const genderSql = gender ? ' AND gender = ?' : '';
    const [[w]] = await this.#pool.execute(
      `SELECT COUNT(*) AS n FROM clothing WHERE user_id = ?${genderSql}`,
      gender ? [userId, gender] : [userId],
    );
    const [regions] = await this.#pool.execute(
      'SELECT name FROM region_setting WHERE user_id = ? ORDER BY updated_at DESC LIMIT 1',
      [userId],
    );
    let moodOk = true;
    if (mood) {
      const [m] = await this.#pool.execute(
        "SELECT 1 FROM style_mood WHERE mood_key = ? AND (gender = ? OR gender = 'unisex') LIMIT 1",
        [mood, gender],
      );
      moodOk = m.length === 1;
    }
    return { wardrobe: Number(w.n), region: regions[0]?.name ?? null, moodOk };
  }

  /** 이 id 들 가운데 이 사용자의 옷인 것. 추천 결과를 내보내기 전에 하나하나 확인한다. */
  async ownedClothingIds(userId, ids) {
    const unique = [...new Set(ids)].filter((id) => typeof id === 'string' && id.length > 0 && id.length <= 64);
    if (unique.length === 0) return new Set();
    const [rows] = await this.#pool.execute(
      `SELECT id FROM clothing WHERE user_id = ? AND id IN (${unique.map(() => '?').join(',')})`,
      [userId, ...unique],
    );
    return new Set(rows.map((r) => r.id));
  }

  /** 추천 한 번에 Rust 가 실제로 쓴 날씨(recommendation_batch.weather_json). 본인 batch 만. */
  async batchWeather(userId, batchId) {
    if (typeof batchId !== 'string' || batchId.length > 64) return null;
    const [rows] = await this.#pool.execute(
      'SELECT weather_json FROM recommendation_batch WHERE id = ? AND user_id = ? LIMIT 1',
      [batchId, userId],
    );
    if (rows.length === 0 || rows[0].weather_json == null) return null;
    const w = typeof rows[0].weather_json === 'string' ? JSON.parse(rows[0].weather_json) : rows[0].weather_json;
    const num = (v) => (typeof v === 'number' && Number.isFinite(v) ? v : null);
    return {
      temperature: num(w.temperature),
      apparent_temperature: num(w.apparent_temperature),
      description: typeof w.weather_description === 'string' ? w.weather_description : null,
    };
  }

  async close() {
    await this.#pool.end();
  }
}

/**
 * 백엔드 응답 타입의 TypeScript 대응.
 *
 * 각 타입은 Rust 쪽 구조체와 1:1로 맞춰져 있고, 아래 주석에 출처를 적어 두었다.
 * Rust 쪽을 바꾸면 여기도 같이 바꿔야 한다 — 지금은 코드 생성 없이 손으로 맞추므로,
 * 어긋나면 런타임에야 드러난다는 점이 이 파일의 알려진 약점이다.
 */

// ─── 표준 어휘 (src/models/style_vocab.rs) ───
// serde 가 표준 문자열로 직렬화하므로 리터럴 유니온으로 그대로 받는다.

export type Role = '베이스' | '포인트' | '약한포인트' | '연결템' | '구조템';
export type Tone = '밝음' | '중간' | '어두움';
export type Saturation = '낮음' | '중간' | '높음';
export type Style = '베이직' | '워크' | '밀리터리' | '포멀' | '스포츠';
export type Weight = '가벼움' | '중간' | '무거움';
export type Thickness = 'thin' | 'medium' | 'thick';

export const CATEGORIES = ['상의', '하의', '아우터', '신발', '가방'] as const;
export type Category = (typeof CATEGORIES)[number];

// ─── 의류 (src/models/clothing.rs :: ClothingResponse) ───

export interface Clothing {
  id: string;
  name: string;
  category: string;
  color: string | null;
  thickness: Thickness;
  image_url: string | null;
  seasons: string[];
  tone: Tone | null;
  saturation: Saturation | null;
  style: Style | null;
  weight: Weight | null;
  role: Role | null;
  color_temperature: string | null;
  versatility: string | null;
  statement_level: number | null;
  formality_level: number | null;
  texture_worlds: string[];
  /** 이 옷에 붙은 장르 전부. 한 벌이 여러 장르에 들어갈 수 있다. */
  style_genres: string[];
  gender: string | null;
  created_at: string;
  updated_at: string;
}

/** CreateClothingRequest 중 UI 가 실제로 보내는 필드만. */
export interface CreateClothingBody {
  name: string;
  category: string;
  thickness: Thickness;
}

export interface ImageUploadBody {
  image_data: string;
}

// ─── 날씨 (src/models/weather.rs :: WeatherResponse) ───

export interface CurrentWeather {
  temperature: number;
  apparent_temperature: number;
  humidity: number;
  wind_speed: number;
  weather_code: number;
  weather_description: string;
}

export interface WeatherResponse {
  region_name: string;
  latitude: number;
  longitude: number;
  current: CurrentWeather;
}

// ─── 지역 (src/models/region.rs :: RegionSetting) ───

export interface Region {
  id: string;
  name: string;
  latitude: number;
  longitude: number;
  updated_at: string;
}

export interface UpsertRegionBody {
  name: string;
  latitude: number;
  longitude: number;
}

// ─── 무드 (src/routes/style_mood.rs :: StyleMood) ───

export interface StyleMood {
  gender: string;
  mood_key: string;
  mood_label: string;
  description: string | null;
}

export type Gender = 'male' | 'female';

// ─── 추천 (src/models/recommendation.rs) ───

export interface OutfitItem {
  category: string;
  name: string;
  reason: string;
  image_url: string | null;
  material: string | null;
}

export interface ScoringDetail {
  style_score: number;
  recency_penalty: number;
  diversity_bonus: number;
  dormant_bonus: number;
  final_score: number;
}

/** mode 값은 백엔드가 정하는 문자열. 알려진 세 가지 외의 값도 안전하게 렌더링한다. */
export type RecommendationMode = 'todays_pick' | 'variation' | 'dormant_revival';

export interface ModeRecommendation {
  mode: string;
  mode_label: string;
  mode_description: string;
  outfit: OutfitItem[];
  recommendation: string;
  weather_summary: string;
  tips: string[];
  score: number;
  verdict: string;
  reason: string;
  revival_items: string[];
  scoring_detail: ScoringDetail | null;
}

export interface MultiModeRecommendation {
  modes: ModeRecommendation[];
  weather_summary: string;
}

export interface RecommendationBody {
  occasion?: string;
  gender?: Gender;
  style_mood?: string | null;
}

// ─── 코디 평가 (src/models/outfit.rs) ───

export type IssueCode =
  | 'TooManyAccents'
  | 'LackOfStructure'
  | 'TooMuchNaturalTone'
  | 'LackOfContrast'
  | 'TextureWorldConflict'
  | 'SeasonalMismatch'
  | 'StrongInner'
  | 'BagConflict'
  | 'StyleConflict'
  | 'FormalitySituationMismatch'
  | 'SlotRoleMismatch'
  | 'WorldOvermatching';

export type Verdict = 'Great' | 'Good' | 'Okay' | 'Awkward';

export interface RuleProblem {
  code: IssueCode;
  rule: string;
  deduction: number;
  detail: string;
}

export interface OutfitStrength {
  rule: string;
  detail: string;
}

export interface StructuredSuggestion {
  type: string;
  reason_code: IssueCode;
  reason: string;
  recommended_roles: string[];
  recommended_colors: string[];
  recommended_examples: string[];
}

export interface EvaluateResponse {
  score: number;
  verdict: Verdict;
  verdict_label: string;
  summary: string;
  problems: RuleProblem[];
  strengths: OutfitStrength[];
  suggestions: StructuredSuggestion[];
  explanation: string;
}

export interface EvaluateBody {
  top?: string;
  bottom?: string;
  outer?: string;
  shoes?: string;
  bag?: string;
  situation?: string;
}

// ─── 채팅 (src/routes/chat.rs) ───

/** 착장 슬롯. 백엔드가 문자열로 주므로 알려진 값 외에도 올 수 있다. */
export type ChatSlot = 'inner' | 'outer' | 'bottom' | 'shoes' | 'bag';

export interface ChatItem {
  slot: string;
  category: string;
  name: string;
  owned: boolean;
  material?: string | null;
}

export interface ChatResponse {
  reply: string;
  items: ChatItem[];
}

export interface ChatBody {
  message: string;
  gender?: Gender;
  style_mood?: string | null;
}

export interface OutfitImageBody {
  /** 백엔드는 문자열 하나를 받는다 (배열이 아님). */
  items: string;
  mood?: string | null;
}

export interface OutfitImageResponse {
  image_url: string | null;
}

// ─── 피드백 (src/models/feedback.rs :: FeedbackRequest) ───

/**
 * 'worn' 과 'saved' 는 서버가 이미 받는 값이다
 * (`db::feedback_repo::insert_feedback` 의 polarity/item_delta 분기).
 * 눌러 본 것과 실제로 입겠다는 것은 다른 신호라 따로 둔다.
 */
export type FeedbackType = 'like' | 'dislike' | 'worn' | 'saved';

export interface FeedbackBody {
  feedback_type: FeedbackType;
  reasons: string[];
  inner_name?: string;
  outer_name?: string;
  bottom_name?: string;
  shoes_name?: string;
  bag_name?: string;
  anchor_name?: string;
  comment?: string;
}


// ─── 룩북 (프런트 전용 — 생성한 코디를 쌓아 둔다) ───

/**
 * 만들어진 코디 한 장.
 *
 * 추천 응답은 모드 셋을 한꺼번에 주지만, 사용자에게는 "코디 한 장" 단위로 쌓인다.
 * 서버에 저장하는 값이 아니라 이번 세션 동안의 화면 상태다.
 */
export interface Look {
  /** 리스트 키. 착장이 같아도 절대 겹치지 않아야 한다 — 겹치면 React 가 노드를
   *  제대로 지우지 못해 썸네일이 하나 더 남는다. */
  key: string;
  /** 중복 판별용 착장 서명. 같은 구성이면 다시 쌓지 않는다. */
  sig: string;
  title: string;
  /** 생성 시각·무드·날씨를 한 줄로. 이전 코디와 오늘 것을 구분한다. */
  meta: string;
  moodKey: string | null;
  outfit: OutfitItem[];
  reason: string;
  recommendation: string;
  liked: boolean;
  worn: boolean;
  /** 서버에 저장된 행의 id. 저장 응답을 받으면 채워진다. */
  savedId?: string;
}

export interface SetGenresBody {
  style_genres: string[];
  gender?: string | null;
}

// ─── 룩북 저장 (src/routes/lookbook.rs) ───

export interface SavedLook {
  id: string;
  mood_key: string | null;
  title: string;
  weather_summary: string | null;
  reason: string | null;
  recommendation: string | null;
  /** 이미지 캐시 키. 이 문자열로 다시 요청하면 같은 그림이 캐시에서 나온다. */
  image_prompt: string;
  outfit_json: OutfitItem[];
  liked: boolean;
  worn: boolean;
  created_at: string;
  /** 이미 만들어 둔 그림. 없으면 아직 생성되지 않은 코디다. */
  image_path: string | null;
}

export interface SaveLookBody {
  mood_key: string | null;
  title: string;
  weather_summary: string | null;
  reason: string | null;
  recommendation: string | null;
  image_prompt: string;
  outfit_json: OutfitItem[];
}

// ─── 헬스체크 (src/routes/health.rs) ───

export interface HealthResponse {
  status: string;
  database: boolean;
}

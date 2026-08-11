use serde::{Deserialize, Serialize};

use crate::models::clothing::Clothing;

/// Request body for outfit evaluation
#[derive(Debug, Deserialize)]
pub struct OutfitEvaluateRequest {
    pub top: Option<String>,
    pub bottom: Option<String>,
    pub outer: Option<String>,
    pub shoes: Option<String>,
    pub bag: Option<String>,
    pub situation: Option<String>,
}

/// Which slot an item occupies
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum SlotKind {
    Top,
    Bottom,
    Outer,
    Shoes,
    Bag,
}

impl SlotKind {
    pub fn label(&self) -> &'static str {
        match self {
            SlotKind::Top => "상의",
            SlotKind::Bottom => "하의",
            SlotKind::Outer => "아우터",
            SlotKind::Shoes => "신발",
            SlotKind::Bag => "가방",
        }
    }

    /// 슬롯별 기대 역할
    pub fn expected_roles(&self, has_outer: bool) -> &'static [&'static str] {
        match self {
            SlotKind::Top if has_outer => &["베이스", "연결템"],
            SlotKind::Top => &["베이스", "포인트", "약한포인트", "연결템"],
            SlotKind::Bottom => &["베이스", "구조템", "연결템"],
            SlotKind::Outer => &["포인트", "약한포인트", "구조템", "연결템"],
            SlotKind::Shoes => &["연결템", "구조템", "베이스"],
            SlotKind::Bag => &["연결템", "구조템", "베이스"],
        }
    }
}

/// A clothing item placed in an outfit slot
#[derive(Debug, Clone)]
pub struct OutfitSlot {
    pub slot: SlotKind,
    pub clothing: Clothing,
    pub seasons: Vec<String>,
    pub texture_worlds: Vec<String>,
}

/// All slots that make up an outfit
pub struct OutfitContext {
    pub slots: Vec<OutfitSlot>,
    pub situation: Option<String>,
}

/// Issue code enum for structured problem identification
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum IssueCode {
    TooManyAccents,
    LackOfStructure,
    TooMuchNaturalTone,
    LackOfContrast,
    TextureWorldConflict,
    SeasonalMismatch,
    StrongInner,
    BagConflict,
    StyleConflict,
    FormalitySituationMismatch,
    SlotRoleMismatch,
    WorldOvermatching,
}

/// A single rule violation
#[derive(Debug, Serialize)]
pub struct RuleProblem {
    pub code: IssueCode,
    pub rule: String,
    pub deduction: i32,
    pub detail: String,
}

/// A detected strength in the outfit
#[derive(Debug, Serialize)]
pub struct OutfitStrength {
    pub rule: String,
    pub detail: String,
}

/// Verdict based on score
#[derive(Debug, Serialize)]
pub enum Verdict {
    Great,
    Good,
    Okay,
    Awkward,
}

impl Verdict {
    pub fn from_score(score: i32) -> Self {
        match score {
            88..=100 => Verdict::Great,
            73..=87 => Verdict::Good,
            55..=72 => Verdict::Okay,
            _ => Verdict::Awkward,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Verdict::Great => "훌륭해요",
            Verdict::Good => "좋아요",
            Verdict::Okay => "괜찮아요",
            Verdict::Awkward => "아쉬워요",
        }
    }
}

/// Structured suggestion for Flutter rendering
#[derive(Debug, Serialize)]
pub struct StructuredSuggestion {
    #[serde(rename = "type")]
    pub suggestion_type: String,
    pub reason_code: IssueCode,
    pub reason: String,
    pub recommended_roles: Vec<String>,
    pub recommended_colors: Vec<String>,
    pub recommended_examples: Vec<String>,
}

/// Result of rule-based evaluation (before LLM explanation)
pub struct EvaluationResult {
    pub score: i32,
    pub verdict: Verdict,
    pub problems: Vec<RuleProblem>,
    pub strengths: Vec<OutfitStrength>,
    pub suggestions: Vec<StructuredSuggestion>,
    pub summary: String,
}

/// Full response returned to client
#[derive(Debug, Serialize)]
pub struct OutfitEvaluateResponse {
    pub score: i32,
    pub verdict: Verdict,
    pub verdict_label: String,
    pub summary: String,
    pub problems: Vec<RuleProblem>,
    pub strengths: Vec<OutfitStrength>,
    pub suggestions: Vec<StructuredSuggestion>,
    pub explanation: String,
}

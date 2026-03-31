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
#[derive(Debug, Clone, Serialize)]
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
            90..=100 => Verdict::Great,
            70..=89 => Verdict::Good,
            50..=69 => Verdict::Okay,
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

/// Result of rule-based evaluation (before LLM explanation)
pub struct EvaluationResult {
    pub score: i32,
    pub verdict: Verdict,
    pub problems: Vec<RuleProblem>,
    pub strengths: Vec<OutfitStrength>,
    pub suggestions: Vec<String>,
}

/// Full response returned to client
#[derive(Debug, Serialize)]
pub struct OutfitEvaluateResponse {
    pub score: i32,
    pub verdict: Verdict,
    pub verdict_label: String,
    pub problems: Vec<RuleProblem>,
    pub strengths: Vec<OutfitStrength>,
    pub suggestions: Vec<String>,
    pub explanation: String,
}

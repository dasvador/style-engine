//! shadow_cases / eval_scorecard 가 공유하는 fixture 로딩 및 변환.
//!
//! 두 테스트 바이너리가 같은 케이스 카탈로그를 읽으므로 스키마가 한 곳에만 있어야 한다.

#![allow(dead_code)]

use std::collections::HashMap;
use std::path::PathBuf;

use chrono::NaiveDateTime;
use serde::Deserialize;

use style_engine::models::clothing::Clothing;
use style_engine::models::outfit::{OutfitContext, OutfitSlot, SlotKind};
use style_engine::models::style_vocab::{Role, Saturation, Style, Tone, Weight};
use style_engine::services::style_engine_v2::HardFilterReason;

// ─── Fixture schema ───

#[derive(Debug, Deserialize)]
pub struct Registry {
    pub items: HashMap<String, RegistryItem>,
}

#[derive(Debug, Deserialize)]
pub struct RegistryItem {
    pub name: String,
    pub category: String,
    pub style: Option<Style>,
    pub role: Option<Role>,
    pub tone: Option<Tone>,
    pub saturation: Option<Saturation>,
    pub color_temperature: Option<String>,
    pub weight: Option<Weight>,
    pub formality_level: Option<i8>,
    #[serde(default)]
    pub seasons: Vec<String>,
    #[serde(default)]
    pub texture_worlds: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct CaseFile {
    #[allow(dead_code)]
    pub version: u32,
    pub cases: Vec<TestCase>,
}

#[derive(Debug, Deserialize)]
pub struct TestCase {
    pub case_id: String,
    pub situation: Option<String>,
    #[serde(default)]
    pub current_season: Option<String>,
    #[allow(dead_code)]
    pub temperature_c: Option<f64>,
    #[serde(default)]
    pub top: String,
    #[serde(default)]
    pub bottom: String,
    #[serde(default)]
    pub outer: String,
    #[serde(default)]
    pub shoes: String,
    #[serde(default)]
    pub bag: String,
    pub expected_hard_pass: bool,
    pub expected_today_fit: String,
    pub expected_preference: String,
    #[serde(default)]
    pub forbidden_reason: String,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub tags: Vec<String>,
}

// ─── Loaders ───

pub fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

pub fn load_registry() -> Registry {
    let content = std::fs::read_to_string(fixture_path("wardrobe_registry.toml"))
        .expect("failed to read wardrobe_registry.toml");
    toml::from_str::<Registry>(&content).expect("failed to parse wardrobe_registry.toml")
}

pub fn load_cases() -> CaseFile {
    let content = std::fs::read_to_string(fixture_path("recommendation_cases.toml"))
        .expect("failed to read recommendation_cases.toml");
    toml::from_str::<CaseFile>(&content).expect("failed to parse recommendation_cases.toml")
}

// ─── Helpers ───

pub fn placeholder_ts() -> NaiveDateTime {
    NaiveDateTime::parse_from_str("2026-04-01 00:00:00", "%Y-%m-%d %H:%M:%S").unwrap()
}

pub fn registry_to_outfit_slot(
    item_id: &str,
    slot_kind: SlotKind,
    registry: &Registry,
) -> Option<OutfitSlot> {
    if item_id.is_empty() {
        return None;
    }
    let item = registry
        .items
        .get(item_id)
        .unwrap_or_else(|| panic!("unknown item '{}' in registry", item_id));
    let now = placeholder_ts();
    Some(OutfitSlot {
        slot: slot_kind,
        clothing: Clothing {
            id: item_id.to_string(),
            name: item.name.clone(),
            category: item.category.clone(),
            color: None,
            thickness: "medium".to_string(),
            image_url: None,
            tone: item.tone,
            saturation: item.saturation,
            style: item.style,
            weight: item.weight,
            role: item.role,
            color_temperature: item.color_temperature.clone(),
            versatility: None,
            statement_level: None,
            formality_level: item.formality_level,
            gender: None,
            style_mood: None,
            visual_weight: None,
            texture_depth: None,
            visual_weight_v2: None,
            texture_depth_v2: None,
            grounding_score: None,
            shadow_tone: None,
            silhouette_volume: None,
            material_primary: None,
            sub_category: None,
            floating_score: None,
            strong_style_score: None,
            texture_keywords: None,
            created_at: now,
            updated_at: now,
        },
        seasons: item.seasons.clone(),
        texture_worlds: item.texture_worlds.clone(),
    })
}

pub fn case_to_context(case: &TestCase, registry: &Registry) -> OutfitContext {
    let slot_defs = [
        (&case.top, SlotKind::Top),
        (&case.bottom, SlotKind::Bottom),
        (&case.outer, SlotKind::Outer),
        (&case.shoes, SlotKind::Shoes),
        (&case.bag, SlotKind::Bag),
    ];
    let slots: Vec<OutfitSlot> = slot_defs
        .iter()
        .filter_map(|(id, kind)| registry_to_outfit_slot(id, *kind, registry))
        .collect();
    OutfitContext {
        slots,
        situation: case.situation.clone(),
    }
}

pub fn reason_code(r: &HardFilterReason) -> &'static str {
    match r {
        HardFilterReason::StyleHardConflict => "StyleHardConflict",
        HardFilterReason::StrongInnerViolation => "StrongInnerViolation",
        HardFilterReason::LackOfStructure => "LackOfStructure",
        HardFilterReason::AllOneTone => "AllOneTone",
        HardFilterReason::SeasonCompleteMismatch => "SeasonCompleteMismatch",
        HardFilterReason::WarmMonotoneNoStructure => "WarmMonotoneNoStructure",
        HardFilterReason::FormalSituationAthleticShoes => "FormalSituationAthleticShoes",
    }
}

pub fn build_outfit_key(c: &TestCase) -> String {
    format!("{}|{}|{}|{}|{}", c.top, c.bottom, c.outer, c.shoes, c.bag)
}

pub fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!(
            "{}…",
            &s[..s
                .char_indices()
                .take(max)
                .last()
                .map(|(i, _)| i)
                .unwrap_or(max)]
        )
    }
}

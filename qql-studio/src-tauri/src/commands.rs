//! Tauri commands exposed to the frontend.

use crate::render::{self, LayoutStats, RenderResult};
use crate::seed::{
    build_seed, matches_filter, parse_address, parse_seed_hex, random_nonce,
    resolve_choices, seed_to_hex, trait_metadata, TraitChoices, TraitMeta, TraitsWire,
};
use once_cell::sync::Lazy;
use qql::art::{FlowFieldSpec, Hsb};
use qql::rand::Rng as QRng;
use qql::traits as q;
use rand::{rngs::StdRng, Rng, RngCore, SeedableRng};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

/// Global cancel flag for the in-flight search. Only one search runs at a time
/// from a single window, so a single flag is sufficient.
static CANCEL: Lazy<AtomicBool> = Lazy::new(|| AtomicBool::new(false));

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    pub seed: String,
    pub traits: TraitsWire,
    pub stats: Option<LayoutStats>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct GenerateRequest {
    pub address: String,
    pub choices: TraitChoices,
    pub mode: GenerateMode,
    pub count: usize,
    pub max_attempts: Option<usize>,
    pub run_layout: bool,
    pub characteristic_filter: Option<CharacteristicFilter>,
    pub rng_seed: Option<u64>,
    /// Number of parallel layout workers. 1 = sequential. Defaults to ~half
    /// the system's logical cores.
    pub workers: Option<usize>,
    /// Lower bound of the flow-line angle in degrees, on the [0, 90] underlying
    /// range (i.e. after undoing the horizontal/vertical flips). Only applies
    /// to seeds with a linear flow field; radial seeds are filtered out when
    /// either bound is set.
    pub min_angle_deg: Option<f64>,
    pub max_angle_deg: Option<f64>,
    /// If `Some`, bail out of any individual layout that takes longer than
    /// this many milliseconds. Skipped layouts still count as attempts.
    pub layout_timeout_ms: Option<u32>,
}

#[derive(Deserialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GenerateMode {
    /// Encode the user's specific trait selections directly; vary the 6-byte
    /// nonce to produce visually distinct seeds with identical categorical traits.
    Construct,
    /// Generate random seeds with the user's address; keep only those whose
    /// decoded traits satisfy the filter.
    Search,
}

#[derive(Deserialize, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct CharacteristicFilter {
    pub min_points: Option<usize>,
    pub max_points: Option<usize>,
    pub min_colors: Option<usize>,
    pub max_colors: Option<usize>,
    pub required_colors: Option<Vec<String>>,
    pub min_small: Option<usize>,
    pub max_small: Option<usize>,
    pub min_medium: Option<usize>,
    pub max_medium: Option<usize>,
    pub min_large: Option<usize>,
    pub max_large: Option<usize>,
    pub min_tl: Option<usize>,
    pub max_tl: Option<usize>,
    pub min_tr: Option<usize>,
    pub max_tr: Option<usize>,
    pub min_bl: Option<usize>,
    pub max_bl: Option<usize>,
    pub min_br: Option<usize>,
    pub max_br: Option<usize>,
    /// Optional corner-concentration constraint. Valid values: "TL", "TR",
    /// "BL", "BR", "AnyLeft", "AnyRight". When set, the densest quadrant must
    /// match this side, and the densest/sparsest ratio must be >= `corner_ratio`.
    pub corner_concentration: Option<String>,
    pub corner_ratio: Option<f64>,
    /// Section-count filters (Formation pieces only). When any of these is
    /// set, non-Formation seeds are rejected automatically.
    pub min_horizontal_sections: Option<usize>,
    pub max_horizontal_sections: Option<usize>,
    pub min_vertical_sections: Option<usize>,
    pub max_vertical_sections: Option<usize>,
    pub min_total_sections: Option<usize>,
    pub max_total_sections: Option<usize>,
    /// Allowed Formation point spacings (`step`) as basis points of canvas
    /// width: 75 = 0.75%, 800 = 8%. Empty / None = no constraint.
    pub allowed_formation_step_bp: Option<Vec<u32>>,
    /// Allowed Formation skip-odds as basis points: 0 = none, 1000 = 0.1,
    /// 2000 = 0.2, 5000 = 0.5. Empty / None = no constraint.
    pub allowed_formation_skip_bp: Option<Vec<u32>>,
    /// Bounds on the number of chunks actually placed (after the skip filter).
    pub min_formation_chunks: Option<usize>,
    pub max_formation_chunks: Option<usize>,
    /// Orbital structure filters (Orbital pieces only). When any is set,
    /// non-Orbital seeds are rejected.
    pub min_ring_bands: Option<usize>,
    pub max_ring_bands: Option<usize>,
    /// If non-empty: no band in the piece may use a split value outside this
    /// set. Empty / None = no constraint.
    pub allowed_orbital_splits: Option<Vec<u32>>,
    /// If non-empty: each listed split value must appear in at least one band.
    /// Empty / None = no constraint.
    pub required_orbital_splits: Option<Vec<u32>>,
    /// Allowed center-X position categories (0=centered, 1=off-center,
    /// 2=just outside, 3=way outside). Empty / None = no constraint.
    pub allowed_orbital_center_x_categories: Option<Vec<u8>>,
    pub allowed_orbital_center_y_categories: Option<Vec<u8>>,
    /// Allowed point spacings (`base_step`) as basis points of canvas width:
    /// e.g. 100 = 1%, 1600 = 16%. Empty / None = no constraint.
    pub allowed_orbital_base_step_bp: Option<Vec<u32>>,
    /// Allowed ring band thicknesses (`radial_group_step`) as basis points.
    /// E.g. 700 = 7%, 3000 = 30%. Empty / None = no constraint.
    pub allowed_orbital_radial_step_bp: Option<Vec<u32>>,
    /// Allowed curve lengths (500 / 650 / 850). Applies to every piece, not
    /// just Orbital. Empty / None = no constraint.
    pub allowed_curve_lengths: Option<Vec<u32>>,
    /// Allowed target circle counts for Shadows pieces. Subset of
    /// `{5, 7, 10, 20, 30, 60}`. Empty / None = no constraint.
    pub allowed_shadows_num_circles: Option<Vec<u32>>,
    /// Shadows fill style preset: "any", "all_radial", "mixed", "all_square".
    /// Maps to p_square = 0.0, 0.5, or 1.0.
    pub shadows_fill_style: Option<String>,
    /// Bounds on the actual circles placed (after collision rejection).
    pub min_shadows_actual_circles: Option<usize>,
    pub max_shadows_actual_circles: Option<usize>,
    /// Subtle booleans, rarely useful.
    pub shadows_columnar_square: Option<bool>,
    pub shadows_outward_radial: Option<bool>,
    /// Bounds on the orbital split offset, in degrees [0, 360). Rarely useful
    /// (purely rotational) but available for completeness.
    pub min_split_offset_deg: Option<f64>,
    pub max_split_offset_deg: Option<f64>,
    /// Splatter preset: "none", "any", "light", "moderate", "heavy". Empty /
    /// None = no constraint. Combines (AND) with min/max splatter odds.
    pub splatter_mode: Option<String>,
    pub min_splatter_odds: Option<f64>,
    pub max_splatter_odds: Option<f64>,
    /// Required background color (exact match by name).
    pub background_color: Option<String>,
    /// Restrict ring colors to this exact set (when `exact_ring_colors` is true)
    /// or require these to be present (when it's false, same as `required_colors`).
    pub ring_colors: Option<Vec<String>>,
    pub exact_ring_colors: Option<bool>,
}

impl CharacteristicFilter {
    fn is_active(&self) -> bool {
        self.min_points.is_some()
            || self.max_points.is_some()
            || self.min_colors.is_some()
            || self.max_colors.is_some()
            || self.min_small.is_some()
            || self.max_small.is_some()
            || self.min_medium.is_some()
            || self.max_medium.is_some()
            || self.min_large.is_some()
            || self.max_large.is_some()
            || self.min_tl.is_some()
            || self.max_tl.is_some()
            || self.min_tr.is_some()
            || self.max_tr.is_some()
            || self.min_bl.is_some()
            || self.max_bl.is_some()
            || self.min_br.is_some()
            || self.max_br.is_some()
            || self.corner_concentration.is_some()
            || self.min_horizontal_sections.is_some()
            || self.max_horizontal_sections.is_some()
            || self.min_vertical_sections.is_some()
            || self.max_vertical_sections.is_some()
            || self.min_total_sections.is_some()
            || self.max_total_sections.is_some()
            || self
                .allowed_formation_step_bp
                .as_ref()
                .map(|v| !v.is_empty())
                .unwrap_or(false)
            || self
                .allowed_formation_skip_bp
                .as_ref()
                .map(|v| !v.is_empty())
                .unwrap_or(false)
            || self.min_formation_chunks.is_some()
            || self.max_formation_chunks.is_some()
            || self.min_ring_bands.is_some()
            || self.max_ring_bands.is_some()
            || self
                .allowed_orbital_splits
                .as_ref()
                .map(|v| !v.is_empty())
                .unwrap_or(false)
            || self
                .required_orbital_splits
                .as_ref()
                .map(|v| !v.is_empty())
                .unwrap_or(false)
            || self
                .allowed_orbital_center_x_categories
                .as_ref()
                .map(|v| !v.is_empty())
                .unwrap_or(false)
            || self
                .allowed_orbital_center_y_categories
                .as_ref()
                .map(|v| !v.is_empty())
                .unwrap_or(false)
            || self
                .allowed_orbital_base_step_bp
                .as_ref()
                .map(|v| !v.is_empty())
                .unwrap_or(false)
            || self
                .allowed_orbital_radial_step_bp
                .as_ref()
                .map(|v| !v.is_empty())
                .unwrap_or(false)
            || self
                .allowed_curve_lengths
                .as_ref()
                .map(|v| !v.is_empty())
                .unwrap_or(false)
            || self.min_split_offset_deg.is_some()
            || self.max_split_offset_deg.is_some()
            || self
                .splatter_mode
                .as_ref()
                .map(|s| !s.is_empty())
                .unwrap_or(false)
            || self.min_splatter_odds.is_some()
            || self.max_splatter_odds.is_some()
            || self
                .allowed_shadows_num_circles
                .as_ref()
                .map(|v| !v.is_empty())
                .unwrap_or(false)
            || self
                .shadows_fill_style
                .as_ref()
                .map(|s| !s.is_empty())
                .unwrap_or(false)
            || self.min_shadows_actual_circles.is_some()
            || self.max_shadows_actual_circles.is_some()
            || self.shadows_columnar_square.is_some()
            || self.shadows_outward_radial.is_some()
            || self.background_color.is_some()
            || self
                .ring_colors
                .as_ref()
                .map(|v| !v.is_empty())
                .unwrap_or(false)
            || self
                .required_colors
                .as_ref()
                .map(|v| !v.is_empty())
                .unwrap_or(false)
    }

    fn matches(&self, stats: &LayoutStats) -> bool {
        fn check_range(value: usize, lo: Option<usize>, hi: Option<usize>) -> bool {
            if let Some(lo) = lo {
                if value < lo {
                    return false;
                }
            }
            if let Some(hi) = hi {
                if value > hi {
                    return false;
                }
            }
            true
        }

        if !check_range(stats.num_points, self.min_points, self.max_points) {
            return false;
        }
        if !check_range(stats.colors_used.len(), self.min_colors, self.max_colors) {
            return false;
        }
        if !check_range(stats.radius_buckets.small, self.min_small, self.max_small) {
            return false;
        }
        if !check_range(
            stats.radius_buckets.medium,
            self.min_medium,
            self.max_medium,
        ) {
            return false;
        }
        if !check_range(stats.radius_buckets.large, self.min_large, self.max_large) {
            return false;
        }
        // Per-quadrant min/max
        let q = &stats.quadrants;
        if !check_range(q.top_left, self.min_tl, self.max_tl) {
            return false;
        }
        if !check_range(q.top_right, self.min_tr, self.max_tr) {
            return false;
        }
        if !check_range(q.bottom_left, self.min_bl, self.max_bl) {
            return false;
        }
        if !check_range(q.bottom_right, self.min_br, self.max_br) {
            return false;
        }
        // Corner-concentration constraint
        if let Some(corner) = &self.corner_concentration {
            let counts = [
                ("TL", q.top_left),
                ("TR", q.top_right),
                ("BL", q.bottom_left),
                ("BR", q.bottom_right),
            ];
            let densest = counts.iter().max_by_key(|(_, n)| *n).unwrap();
            let sparsest = counts.iter().min_by_key(|(_, n)| *n).unwrap();
            let densest_ok = match corner.as_str() {
                "TL" => densest.0 == "TL",
                "TR" => densest.0 == "TR",
                "BL" => densest.0 == "BL",
                "BR" => densest.0 == "BR",
                "AnyLeft" => densest.0 == "TL" || densest.0 == "BL",
                "AnyRight" => densest.0 == "TR" || densest.0 == "BR",
                _ => true,
            };
            if !densest_ok {
                return false;
            }
            if let Some(ratio_min) = self.corner_ratio {
                if sparsest.1 == 0 {
                    if densest.1 == 0 {
                        return false;
                    }
                    // Sparsest is zero => infinite ratio, treat as pass.
                } else {
                    let ratio = densest.1 as f64 / sparsest.1 as f64;
                    if ratio < ratio_min {
                        return false;
                    }
                }
            }
        }
        if let Some(needed) = &self.required_colors {
            for c in needed {
                if !stats.colors_used.iter().any(|u| u == c) {
                    return false;
                }
            }
        }
        let non_empty = |v: &Option<Vec<u8>>| v.as_ref().map(|x| !x.is_empty()).unwrap_or(false);
        let non_empty_u32 =
            |v: &Option<Vec<u32>>| v.as_ref().map(|x| !x.is_empty()).unwrap_or(false);
        // Compare a fraction against an allowed basis-points set with rounding
        // tolerance, to dodge floating-point equality issues.
        let bp_matches = |frac: f64, allowed: &[u32]| {
            let bp = (frac * 10_000.0).round() as i64;
            allowed.iter().any(|w| (*w as i64 - bp).abs() <= 10)
        };
        let section_filter_active = self.min_horizontal_sections.is_some()
            || self.max_horizontal_sections.is_some()
            || self.min_vertical_sections.is_some()
            || self.max_vertical_sections.is_some()
            || self.min_total_sections.is_some()
            || self.max_total_sections.is_some()
            || non_empty_u32(&self.allowed_formation_step_bp)
            || non_empty_u32(&self.allowed_formation_skip_bp)
            || self.min_formation_chunks.is_some()
            || self.max_formation_chunks.is_some();
        if section_filter_active {
            // Only Formation pieces have a section grid; reject everything else.
            let Some(dims) = &stats.formation_dims else {
                return false;
            };
            if !check_range(
                dims.horizontal,
                self.min_horizontal_sections,
                self.max_horizontal_sections,
            ) {
                return false;
            }
            if !check_range(
                dims.vertical,
                self.min_vertical_sections,
                self.max_vertical_sections,
            ) {
                return false;
            }
            if !check_range(
                dims.total,
                self.min_total_sections,
                self.max_total_sections,
            ) {
                return false;
            }
            if let Some(allowed) = self.allowed_formation_step_bp.as_ref() {
                if !allowed.is_empty() && !bp_matches(dims.step_frac, allowed) {
                    return false;
                }
            }
            if let Some(allowed) = self.allowed_formation_skip_bp.as_ref() {
                if !allowed.is_empty() && !bp_matches(dims.skip_odds, allowed) {
                    return false;
                }
            }
            if !check_range(
                dims.actual_chunks,
                self.min_formation_chunks,
                self.max_formation_chunks,
            ) {
                return false;
            }
        }
        let orbital_filter_active = self.min_ring_bands.is_some()
            || self.max_ring_bands.is_some()
            || non_empty_u32(&self.allowed_orbital_splits)
            || non_empty_u32(&self.required_orbital_splits)
            || non_empty(&self.allowed_orbital_center_x_categories)
            || non_empty(&self.allowed_orbital_center_y_categories)
            || non_empty_u32(&self.allowed_orbital_base_step_bp)
            || non_empty_u32(&self.allowed_orbital_radial_step_bp)
            || self.min_split_offset_deg.is_some()
            || self.max_split_offset_deg.is_some();
        if orbital_filter_active {
            let Some(info) = &stats.orbital_info else {
                return false;
            };
            if !check_range(info.ring_bands, self.min_ring_bands, self.max_ring_bands) {
                return false;
            }
            if let Some(allowed) = self.allowed_orbital_splits.as_ref() {
                if !allowed.is_empty() {
                    for used in &info.splits_used {
                        if !allowed.contains(used) {
                            return false;
                        }
                    }
                }
            }
            if let Some(required) = self.required_orbital_splits.as_ref() {
                if !required.is_empty() {
                    for need in required {
                        if !info.splits_used.contains(need) {
                            return false;
                        }
                    }
                }
            }
            if let Some(allowed) = self.allowed_orbital_center_x_categories.as_ref() {
                if !allowed.is_empty() && !allowed.contains(&info.center_x_category) {
                    return false;
                }
            }
            if let Some(allowed) = self.allowed_orbital_center_y_categories.as_ref() {
                if !allowed.is_empty() && !allowed.contains(&info.center_y_category) {
                    return false;
                }
            }
            // Compare fractions via basis-points integer rounding to dodge
            // floating-point equality headaches.
            let frac_to_bp = |f: f64| (f * 10_000.0).round() as i64;
            if let Some(allowed) = self.allowed_orbital_base_step_bp.as_ref() {
                if !allowed.is_empty() {
                    let bp = frac_to_bp(info.base_step_frac);
                    if !allowed.iter().any(|w| (*w as i64 - bp).abs() <= 10) {
                        return false;
                    }
                }
            }
            if let Some(allowed) = self.allowed_orbital_radial_step_bp.as_ref() {
                if !allowed.is_empty() {
                    let bp = frac_to_bp(info.radial_group_step_frac);
                    if !allowed.iter().any(|w| (*w as i64 - bp).abs() <= 10) {
                        return false;
                    }
                }
            }
            if let Some(lo) = self.min_split_offset_deg {
                if info.split_offset_deg < lo {
                    return false;
                }
            }
            if let Some(hi) = self.max_split_offset_deg {
                if info.split_offset_deg > hi {
                    return false;
                }
            }
        }
        if let Some(allowed) = self.allowed_curve_lengths.as_ref() {
            if !allowed.is_empty() && !allowed.contains(&(stats.curve_length as u32)) {
                return false;
            }
        }
        let shadows_filter_active = self
            .allowed_shadows_num_circles
            .as_ref()
            .map(|v| !v.is_empty())
            .unwrap_or(false)
            || self
                .shadows_fill_style
                .as_ref()
                .map(|s| !s.is_empty())
                .unwrap_or(false)
            || self.min_shadows_actual_circles.is_some()
            || self.max_shadows_actual_circles.is_some()
            || self.shadows_columnar_square.is_some()
            || self.shadows_outward_radial.is_some();
        if shadows_filter_active {
            let Some(info) = &stats.shadows_info else {
                return false;
            };
            if let Some(allowed) = self.allowed_shadows_num_circles.as_ref() {
                if !allowed.is_empty() && !allowed.contains(&info.num_circles_target) {
                    return false;
                }
            }
            if let Some(style) = self.shadows_fill_style.as_ref() {
                let eps = 0.05;
                let ok = match style.as_str() {
                    "" => true,
                    "all_radial" => info.p_square.abs() < eps,
                    "mixed" => (info.p_square - 0.5).abs() < eps,
                    "all_square" => (info.p_square - 1.0).abs() < eps,
                    _ => true,
                };
                if !ok {
                    return false;
                }
            }
            if !check_range(
                info.actual_circles,
                self.min_shadows_actual_circles,
                self.max_shadows_actual_circles,
            ) {
                return false;
            }
            if let Some(want) = self.shadows_columnar_square {
                if info.columnar_square != want {
                    return false;
                }
            }
            if let Some(want) = self.shadows_outward_radial {
                if info.outward_radial != want {
                    return false;
                }
            }
        }
        // Splatter preset + numeric bounds. Both apply (AND) when set.
        if let Some(mode) = self.splatter_mode.as_ref() {
            let odds = stats.splatter_odds;
            let ok = match mode.as_str() {
                "" => true,
                "none" => odds <= 0.0,
                "any" => odds > 0.0,
                "light" => odds > 0.0 && odds <= 0.005,
                "moderate" => odds >= 0.01,
                "heavy" => odds >= 0.08,
                _ => true,
            };
            if !ok {
                return false;
            }
        }
        if let Some(lo) = self.min_splatter_odds {
            if stats.splatter_odds < lo {
                return false;
            }
        }
        if let Some(hi) = self.max_splatter_odds {
            if stats.splatter_odds > hi {
                return false;
            }
        }
        if let Some(want_bg) = &self.background_color {
            if &stats.background_color != want_bg {
                return false;
            }
        }
        if let Some(ring) = &self.ring_colors {
            if !ring.is_empty() {
                let exact = self.exact_ring_colors.unwrap_or(false);
                // All required colors must be present.
                for c in ring {
                    if !stats.colors_used.iter().any(|u| u == c) {
                        return false;
                    }
                }
                if exact {
                    // No colors outside the allowed set may appear.
                    for c in &stats.colors_used {
                        if !ring.iter().any(|r| r == c) {
                            return false;
                        }
                    }
                }
            }
        }
        true
    }
}

#[tauri::command]
pub fn list_traits() -> Vec<TraitMeta> {
    trait_metadata()
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ColorInfo {
    pub name: String,
    pub swatch: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PaletteColors {
    pub palette: String,
    pub backgrounds: Vec<ColorInfo>,
    pub primaries: Vec<ColorInfo>,
}

fn color_info(db: &qql::color::ColorDb, key: u32) -> ColorInfo {
    let c = db.color(key).expect("color in db");
    let rgb = Hsb(c.hue, c.sat, c.bright).to_rgb();
    let r = rgb.0.clamp(0.0, 255.0) as u8;
    let g = rgb.1.clamp(0.0, 255.0) as u8;
    let b = rgb.2.clamp(0.0, 255.0) as u8;
    ColorInfo {
        name: c.name.clone(),
        swatch: format!("#{:02x}{:02x}{:02x}", r, g, b),
    }
}

#[tauri::command]
pub fn list_palette_colors() -> Vec<PaletteColors> {
    use q::ColorPalette::*;
    let db = render::color_db();
    let palettes = [Austin, Berlin, Edinburgh, Fidenza, Miami, Seattle, Seoul];
    palettes
        .iter()
        .map(|p| {
            let spec = db.palette(*p).expect("palette exists");
            let backgrounds = spec
                .background_colors
                .iter()
                .map(|(bg, _w)| color_info(db, bg.color))
                .collect();
            let primaries = spec
                .color_seq
                .iter()
                .map(|k| color_info(db, *k))
                .collect();
            PaletteColors {
                palette: format!("{:?}", p),
                backgrounds,
                primaries,
            }
        })
        .collect()
}

#[tauri::command]
pub fn decode_seed(seed_hex: String) -> Result<TraitsWire, String> {
    let bytes = parse_seed_hex(&seed_hex).map_err(|e| e.to_string())?;
    let traits = q::Traits::from_seed(&bytes);
    Ok(TraitsWire::from(&traits))
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GenerateProgress {
    pub attempts: usize,
    pub found: usize,
    pub max_attempts: usize,
    pub want: usize,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GenerateOutcome {
    pub candidates: Vec<Candidate>,
    pub attempts: usize,
    pub max_attempts: usize,
    pub cancelled: bool,
}

#[tauri::command]
pub async fn generate_candidates(
    app: AppHandle,
    request: GenerateRequest,
) -> Result<GenerateOutcome, String> {
    CANCEL.store(false, Ordering::SeqCst);
    let app_clone = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        generate_candidates_inner(request, move |progress| {
            let _ = app_clone.emit("search-progress", progress);
        })
    })
    .await
    .map_err(|e| format!("worker join error: {e}"))?;
    result.map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub fn cancel_search() {
    CANCEL.store(true, Ordering::SeqCst);
}

/// Compute the underlying linear flow-line angle (in degrees, range [0, 90])
/// for a seed whose flow field is linear. Returns `None` for radial flow
/// fields. The "underlying" angle is the value before QQL applies its
/// horizontal/vertical flip — visually those flips don't change the line
/// orientation, so 226° and 46° look identical.
fn linear_flow_angle_deg(seed: &[u8; 32], traits: &q::Traits) -> Option<f64> {
    use q::FlowField::*;
    match traits.flow_field {
        Horizontal | Diagonal | Vertical | RandomLinear => {}
        Explosive | Spiral | Circular | RandomRadial => return None,
    }
    let mut rng = QRng::from_seed(&seed[..]);
    let spec = FlowFieldSpec::from_traits(traits, &mut rng);
    let theta = match spec {
        FlowFieldSpec::Linear { default_theta } => default_theta,
        FlowFieldSpec::Radial { .. } => return None,
    };
    let pi = std::f64::consts::PI;
    let half_pi = std::f64::consts::FRAC_PI_2;
    // Reduce to [0, pi) by ignoring the up/down flip…
    let mut t = theta.rem_euclid(pi);
    // …then mirror anything past pi/2 (that's the left/right flip).
    if t > half_pi {
        t = pi - t;
    }
    Some(t.to_degrees())
}

fn angle_filter_active(min: Option<f64>, max: Option<f64>) -> bool {
    let min_v = min.unwrap_or(0.0);
    let max_v = max.unwrap_or(90.0);
    min_v > 0.0 || max_v < 90.0
}

fn angle_passes(angle: Option<f64>, min: Option<f64>, max: Option<f64>) -> bool {
    let min_v = min.unwrap_or(0.0);
    let max_v = max.unwrap_or(90.0);
    match angle {
        Some(a) => a >= min_v && a <= max_v,
        // Radial seeds can't satisfy an angle range, so reject them when one
        // is set.
        None => false,
    }
}

fn default_worker_count() -> usize {
    let cores = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);
    // Leave a couple of cores free for the OS / other apps.
    cores.saturating_sub(2).max(2)
}

/// Generate a single candidate seed (without running layout). Pure function over
/// the provided RNG so it can be called sequentially before parallel layout.
fn make_seed(
    request: &GenerateRequest,
    address: &[u8; 20],
    rng: &mut StdRng,
) -> anyhow::Result<[u8; 32]> {
    let nonce = random_nonce(rng);
    match request.mode {
        GenerateMode::Construct => {
            let resolved = resolve_choices(&request.choices, rng)?;
            build_seed(address, &nonce, &resolved)
        }
        GenerateMode::Search => {
            let mut s = [0u8; 32];
            s[0..20].copy_from_slice(address);
            s[20..26].copy_from_slice(&nonce);
            s[26] = 0xff;
            s[27] = 0xff;
            let raw: u32 = rng.gen();
            let raw = (raw & 0x0fff_ffff) | (1u32 << 28);
            s[28..32].copy_from_slice(&raw.to_be_bytes());
            Ok(s)
        }
    }
}

fn generate_candidates_inner<F>(
    request: GenerateRequest,
    on_progress: F,
) -> anyhow::Result<GenerateOutcome>
where
    F: Fn(GenerateProgress) + Send + Sync + 'static,
{
    let address = parse_address(&request.address)?;
    let seed_for_rng = request.rng_seed.unwrap_or_else(|| rand::random());
    let mut rng = StdRng::seed_from_u64(seed_for_rng);

    let want = request.count.clamp(1, 2048);
    let max_attempts = request
        .max_attempts
        .unwrap_or(want.saturating_mul(200).max(1000))
        .max(want);
    let char_filter = request
        .characteristic_filter
        .as_ref()
        .filter(|f| f.is_active());
    let needs_layout = request.run_layout || char_filter.is_some();
    let angle_active = angle_filter_active(request.min_angle_deg, request.max_angle_deg);
    let min_angle = request.min_angle_deg;
    let max_angle = request.max_angle_deg;
    let layout_timeout = request
        .layout_timeout_ms
        .filter(|ms| *ms > 0)
        .map(|ms| std::time::Duration::from_millis(ms as u64));

    let workers = request
        .workers
        .filter(|w| (1..=64).contains(w))
        .unwrap_or_else(default_worker_count);
    // One item per worker per batch. Larger batches give marginally better
    // throughput but at this size each Cancel only needs to wait for one
    // layout per worker to finish, which feels much snappier.
    let batch_size = workers.max(4);

    let mut results: Vec<Candidate> = Vec::with_capacity(want);
    let mut attempts = 0usize;
    let mut cancelled = false;

    // Atomics shared with the progress-emitter thread so it can broadcast the
    // live counts without coordinating with the work loop. `*_atomic` holds the
    // count from completed batches; `inflight_*` ticks up inside parallel
    // workers so the UI sees progress while a batch is mid-flight.
    let attempts_atomic = Arc::new(AtomicUsize::new(0));
    let found_atomic = Arc::new(AtomicUsize::new(0));
    let inflight_attempts = Arc::new(AtomicUsize::new(0));
    let inflight_found = Arc::new(AtomicUsize::new(0));
    let emitter_done = Arc::new(AtomicBool::new(false));
    let on_progress = Arc::new(on_progress);

    // Spawn a dedicated emitter so the UI sees regular updates even if the
    // search itself runs in tight, sub-millisecond loops.
    let emitter_handle = {
        let attempts_atomic = attempts_atomic.clone();
        let found_atomic = found_atomic.clone();
        let inflight_attempts = inflight_attempts.clone();
        let inflight_found = inflight_found.clone();
        let emitter_done = emitter_done.clone();
        let on_progress = on_progress.clone();
        std::thread::Builder::new()
            .name("qql-progress".into())
            .spawn(move || {
                on_progress(GenerateProgress {
                    attempts: 0,
                    found: 0,
                    max_attempts,
                    want,
                });
                while !emitter_done.load(Ordering::Relaxed) {
                    std::thread::sleep(Duration::from_millis(60));
                    let attempts =
                        attempts_atomic.load(Ordering::Relaxed)
                            + inflight_attempts.load(Ordering::Relaxed);
                    let found = std::cmp::min(
                        want,
                        found_atomic.load(Ordering::Relaxed)
                            + inflight_found.load(Ordering::Relaxed),
                    );
                    on_progress(GenerateProgress {
                        attempts,
                        found,
                        max_attempts,
                        want,
                    });
                }
            })
            .expect("failed to spawn progress emitter")
    };

    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(workers)
        .thread_name(|i| format!("qql-search-{i}"))
        .build()
        .map_err(|e| anyhow::anyhow!("failed to build thread pool: {e}"))?;

    while results.len() < want && attempts < max_attempts {
        if CANCEL.load(Ordering::SeqCst) {
            cancelled = true;
            break;
        }

        let remaining_attempts = max_attempts - attempts;
        let this_batch = batch_size.min(remaining_attempts);
        if this_batch == 0 {
            break;
        }

        let mut batch: Vec<[u8; 32]> = Vec::with_capacity(this_batch);
        for _ in 0..this_batch {
            batch.push(make_seed(&request, &address, &mut rng)?);
        }

        // Reset per-batch counters so the emitter sees a fresh in-flight tally.
        inflight_attempts.store(0, Ordering::Relaxed);
        inflight_found.store(0, Ordering::Relaxed);

        let batch_results: Vec<Option<Candidate>> = {
            let inflight_attempts = &inflight_attempts;
            let inflight_found = &inflight_found;
            pool.install(|| {
                batch
                    .par_iter()
                    .map(|seed| {
                        // Fast bail when the user has hit Cancel. Each worker
                        // still has to finish the layout it's currently on, but
                        // subsequent items in its slice are skipped.
                        if CANCEL.load(Ordering::Relaxed) {
                            return None;
                        }
                        let traits = q::Traits::from_seed(seed);
                        if !matches_filter(&traits, &request.choices) {
                            inflight_attempts.fetch_add(1, Ordering::Relaxed);
                            return None;
                        }
                        if angle_active {
                            let angle = linear_flow_angle_deg(seed, &traits);
                            if !angle_passes(angle, min_angle, max_angle) {
                                inflight_attempts.fetch_add(1, Ordering::Relaxed);
                                return None;
                            }
                        }
                        let stats = if needs_layout {
                            let deadline =
                                layout_timeout.map(|d| std::time::Instant::now() + d);
                            match render::layout_only_with_deadline(seed, deadline) {
                                Some(s) => Some(s),
                                None => {
                                    // Timed out — count the attempt and move on.
                                    inflight_attempts.fetch_add(1, Ordering::Relaxed);
                                    return None;
                                }
                            }
                        } else {
                            None
                        };
                        if let (Some(filter), Some(s)) = (char_filter, stats.as_ref()) {
                            if !filter.matches(s) {
                                inflight_attempts.fetch_add(1, Ordering::Relaxed);
                                return None;
                            }
                        }
                        inflight_attempts.fetch_add(1, Ordering::Relaxed);
                        inflight_found.fetch_add(1, Ordering::Relaxed);
                        Some(Candidate {
                            seed: seed_to_hex(seed),
                            traits: TraitsWire::from(&traits),
                            stats,
                        })
                    })
                    .collect()
            })
        };

        // Commit the in-flight counters into the running totals. Use the
        // actual evaluated count so cancelled batches don't inflate `attempts`.
        let batch_attempts = inflight_attempts.load(Ordering::Relaxed);
        attempts += batch_attempts;
        attempts_atomic.store(attempts, Ordering::Relaxed);
        for hit in batch_results.into_iter().flatten() {
            if results.len() >= want {
                break;
            }
            results.push(hit);
        }
        found_atomic.store(results.len(), Ordering::Relaxed);
        inflight_attempts.store(0, Ordering::Relaxed);
        inflight_found.store(0, Ordering::Relaxed);
    }

    // Stop the emitter and flush a definitive final state.
    emitter_done.store(true, Ordering::Relaxed);
    let _ = emitter_handle.join();
    on_progress(GenerateProgress {
        attempts,
        found: results.len(),
        max_attempts,
        want,
    });

    Ok(GenerateOutcome {
        candidates: results,
        attempts,
        max_attempts,
        cancelled,
    })
}

#[tauri::command]
pub async fn render_seed(seed_hex: String, width: i32) -> Result<RenderResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let bytes = parse_seed_hex(&seed_hex).map_err(|e| e.to_string())?;
        let w = width.clamp(64, 9600);
        render::render(&bytes, w).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("worker join error: {e}"))?
}

/// Render a seed at `width` and write it as `<folder>/<seed-hex>.png` (no "0x"
/// prefix in the filename). Returns the full path written. Used by the favorite
/// (heart) action.
#[tauri::command]
pub async fn save_seed_png(
    seed_hex: String,
    width: i32,
    folder: String,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let bytes = parse_seed_hex(&seed_hex).map_err(|e| e.to_string())?;
        let w = width.clamp(64, 9600);
        let png = render::render_png_bytes(&bytes, w).map_err(|e| e.to_string())?;
        let name = format!("{}.png", seed_hex.trim_start_matches("0x"));
        let path = std::path::Path::new(&folder).join(name);
        std::fs::write(&path, &png).map_err(|e| format!("write failed: {e}"))?;
        Ok(path.to_string_lossy().into_owned())
    })
    .await
    .map_err(|e| format!("worker join error: {e}"))?
}

/// Delete a previously-saved favorite PNG (un-hearting). Missing file is treated
/// as success so the UI stays consistent.
#[tauri::command]
pub fn delete_seed_png(folder: String, seed_hex: String) -> Result<(), String> {
    let name = format!("{}.png", seed_hex.trim_start_matches("0x"));
    let path = std::path::Path::new(&folder).join(name);
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("delete failed: {e}")),
    }
}

/// Render a seed at `width` and write it to an exact destination `path` chosen
/// by the user (the Detail view's "Save As…"). Returns the path written.
#[tauri::command]
pub async fn export_seed_png(
    seed_hex: String,
    width: i32,
    path: String,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let bytes = parse_seed_hex(&seed_hex).map_err(|e| e.to_string())?;
        let w = width.clamp(64, 9600);
        let png = render::render_png_bytes(&bytes, w).map_err(|e| e.to_string())?;
        std::fs::write(&path, &png).map_err(|e| format!("write failed: {e}"))?;
        Ok(path)
    })
    .await
    .map_err(|e| format!("worker join error: {e}"))?
}

#[tauri::command]
pub async fn layout_summary(seed_hex: String) -> Result<LayoutStats, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let bytes = parse_seed_hex(&seed_hex).map_err(|e| e.to_string())?;
        Ok(render::layout_only(&bytes))
    })
    .await
    .map_err(|e| format!("worker join error: {e}"))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine as _;

    #[test]
    fn construct_mode_produces_matching_traits() {
        let mut choices = TraitChoices::default();
        choices.color_palette = Some("Edinburgh".into());
        choices.structure = Some("Orbital".into());
        choices.flow_field = Some("Spiral".into());
        let req = GenerateRequest {
            address: "0x33c9371d25ce44a408f8a6473fbad86bf81e1a17".into(),
            choices: choices.clone(),
            mode: GenerateMode::Construct,
            count: 4,
            max_attempts: Some(20),
            run_layout: false,
            characteristic_filter: None,
            rng_seed: Some(42),
            workers: Some(1),
            min_angle_deg: None,
            max_angle_deg: None,
            layout_timeout_ms: None,
        };
        let outcome = generate_candidates_inner(req, |_| {}).unwrap();
        assert_eq!(outcome.candidates.len(), 4);
        assert!(!outcome.cancelled);
        for c in &outcome.candidates {
            assert_eq!(c.traits.color_palette, "Edinburgh");
            assert_eq!(c.traits.structure, "Orbital");
            assert_eq!(c.traits.flow_field, "Spiral");
        }
        let mut seen = std::collections::HashSet::new();
        for c in &outcome.candidates {
            assert!(seen.insert(c.seed.clone()), "duplicate seed: {}", c.seed);
        }
    }

    #[test]
    fn linear_flow_angle_matches_known_seeds() {
        // Reference values come from `examples/decode.rs`; underlying angles
        // computed by undoing the horizontal/vertical flips.
        let cases: &[(&str, f64)] = &[
            (
                "8814ce9a6715f8a1428f14fc9f57601d1d5bd653ab2bba65f416ffff1eed4b53",
                45.66,
            ),
            (
                "44266f38ea9ef4e85a77310518b1cb6d5a56349ba9fb12aa9366ffff10a1c943",
                54.0,
            ),
            (
                "8f1c8e9390e10f91de65d0cb594464bbb6cb0aa5c770e4913186ffff10c10c53",
                43.7,
            ),
        ];
        for (hex_str, expected) in cases {
            let bytes: [u8; 32] = hex::decode(hex_str).unwrap().try_into().unwrap();
            let traits = q::Traits::from_seed(&bytes);
            let angle = linear_flow_angle_deg(&bytes, &traits).expect("linear seed");
            let diff = (angle - expected).abs();
            assert!(
                diff < 1.0,
                "seed {} expected ~{}°, got {}°",
                hex_str,
                expected,
                angle
            );
        }
    }

    #[test]
    fn render_returns_png_bytes() {
        let result = render::render(
            &hex_literal::hex!(
                "33c9371d25ce44a408f8a6473fbad86bf81e1a17a2e52c90cf66ffff1296712e"
            ),
            240,
        )
        .unwrap();
        assert!(result.png_data_url.starts_with("data:image/png;base64,"));
        // Decoded payload must be a non-trivial PNG: at least PNG header + data.
        let b64 = result.png_data_url.trim_start_matches("data:image/png;base64,");
        let decoded = base64::engine::general_purpose::STANDARD.decode(b64).unwrap();
        assert!(decoded.len() > 1000, "tiny PNG: {}", decoded.len());
        assert_eq!(&decoded[0..8], b"\x89PNG\r\n\x1a\n");
        assert!(result.stats.num_points > 0);
    }
}

#[tauri::command]
pub fn random_seed_for_address(address: String) -> Result<String, String> {
    let addr = parse_address(&address).map_err(|e| e.to_string())?;
    let mut rng = StdRng::from_entropy();
    let mut s = [0u8; 32];
    s[0..20].copy_from_slice(&addr);
    rng.fill_bytes(&mut s[20..26]);
    s[26] = 0xff;
    s[27] = 0xff;
    let raw: u32 = rng.gen();
    let raw = (raw & 0x0fff_ffff) | (1u32 << 28);
    s[28..32].copy_from_slice(&raw.to_be_bytes());
    Ok(seed_to_hex(&s))
}


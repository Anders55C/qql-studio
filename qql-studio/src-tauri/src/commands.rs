//! Tauri commands exposed to the frontend.

use crate::render::{self, LayoutStats, RenderResult, StructureStats};
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
    /// Spiral center filter: bounds on where a radial flow field's center
    /// sits, as fractions of canvas width / height (centers range from −⅔ to
    /// 5⁄3, so they can be well off-canvas). Linear seeds are filtered out
    /// when any bound is set.
    pub min_center_x: Option<f64>,
    pub max_center_x: Option<f64>,
    pub min_center_y: Option<f64>,
    pub max_center_y: Option<f64>,
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

#[derive(Deserialize, Debug, Default, Clone)]
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
    /// Allowed Orbital center positions per axis, as position codes -3..=3
    /// (see `OrbitalInfoWire::center_x_position`). Empty / None = any.
    pub allowed_orbital_center_x_positions: Option<Vec<i8>>,
    pub allowed_orbital_center_y_positions: Option<Vec<i8>>,
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
    /// Allowed background colors (exact names). The piece's background must
    /// be one of these. Empty / None = any background.
    pub background_colors: Option<Vec<String>>,
    /// Restrict ring colors to this exact set (when `exact_ring_colors` is true)
    /// or require these to be present (when it's false, same as `required_colors`).
    pub ring_colors: Option<Vec<String>>,
    pub exact_ring_colors: Option<bool>,
}

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
                .allowed_orbital_center_x_positions
                .as_ref()
                .map(|v| !v.is_empty())
                .unwrap_or(false)
            || self
                .allowed_orbital_center_y_positions
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
            || self
                .background_colors
                .as_ref()
                .map(|v| !v.is_empty())
                .unwrap_or(false)
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

    /// Filters that read point-level stats, so need a full layout.
    fn points_active(&self) -> bool {
        let non_empty = |v: &Option<Vec<String>>| v.as_ref().map(|x| !x.is_empty()).unwrap_or(false);
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
            || non_empty(&self.required_colors)
            || non_empty(&self.ring_colors)
    }

    /// Filters that only read values fixed before any dots are placed
    /// (structure, curve length, splatter, background) — checkable without a
    /// layout via [`render::structure_only`].
    fn structure_active(&self) -> bool {
        // Everything `is_active` covers minus the point-level filters, done
        // by clearing those and re-asking `is_active`, so a newly added
        // structure field can't be silently skipped.
        let probe = CharacteristicFilter {
            min_points: None,
            max_points: None,
            min_colors: None,
            max_colors: None,
            required_colors: None,
            min_small: None,
            max_small: None,
            min_medium: None,
            max_medium: None,
            min_large: None,
            max_large: None,
            min_tl: None,
            max_tl: None,
            min_tr: None,
            max_tr: None,
            min_bl: None,
            max_bl: None,
            min_br: None,
            max_br: None,
            corner_concentration: None,
            corner_ratio: None,
            ring_colors: None,
            exact_ring_colors: None,
            ..self.clone()
        };
        probe.is_active()
    }

    fn matches_points(&self, stats: &LayoutStats) -> bool {
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

    fn matches_structure(&self, stats: &StructureStats) -> bool {
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
            || self.allowed_orbital_center_x_positions.as_ref().is_some_and(|v| !v.is_empty())
            || self.allowed_orbital_center_y_positions.as_ref().is_some_and(|v| !v.is_empty())
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
            if let Some(allowed) = self.allowed_orbital_center_x_positions.as_ref() {
                if !allowed.is_empty() && !allowed.contains(&info.center_x_position) {
                    return false;
                }
            }
            if let Some(allowed) = self.allowed_orbital_center_y_positions.as_ref() {
                if !allowed.is_empty() && !allowed.contains(&info.center_y_position) {
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
        if let Some(allowed) = &self.background_colors {
            if !allowed.is_empty() && !allowed.contains(&stats.background_color) {
                return false;
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
pub struct BackgroundInfo {
    pub name: String,
    pub swatch: String,
    /// Chance this background is picked for a piece in its palette (0–1).
    pub odds: f64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PaletteColors {
    pub palette: String,
    pub backgrounds: Vec<BackgroundInfo>,
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
            let total: f64 = spec.background_colors.iter().map(|(_, w)| *w as f64).sum();
            let backgrounds = spec
                .background_colors
                .iter()
                .map(|(bg, w)| {
                    let ColorInfo { name, swatch } = color_info(db, bg.color);
                    BackgroundInfo {
                        name,
                        swatch,
                        odds: *w as f64 / total,
                    }
                })
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
    /// Attempts abandoned because their layout hit the layout timeout.
    pub timed_out: usize,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GenerateOutcome {
    pub candidates: Vec<Candidate>,
    pub attempts: usize,
    pub max_attempts: usize,
    pub cancelled: bool,
    /// How many of `attempts` were abandoned at the layout timeout (they
    /// count as misses, but may have matched given more time).
    pub timed_out: usize,
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

/// Random id for this run of the app. The webview can be killed and reloaded
/// by WebKit (e.g. over its memory limit while hidden) while the app keeps
/// running; the page compares this id with the one it saved to tell "the view
/// reloaded" (restore the results) from "the app was relaunched" (start fresh).
static SESSION_ID: Lazy<u64> = Lazy::new(rand::random);

#[tauri::command]
pub fn session_id() -> String {
    format!("{:016x}", *SESSION_ID)
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
    // Slack so seeds computed as e.g. 45.00000000000005° (Diagonal, after a
    // flip) still pass a range ending at 45.
    const EPS: f64 = 1e-6;
    match angle {
        Some(a) => a >= min_v - EPS && a <= max_v + EPS,
        // Radial seeds can't satisfy an angle range, so reject them when one
        // is set.
        None => false,
    }
}

#[derive(Clone, Copy, Debug)]
struct CenterBounds {
    min_x: f64,
    max_x: f64,
    min_y: f64,
    max_y: f64,
}

impl CenterBounds {
    fn from_request(r: &GenerateRequest) -> Option<Self> {
        let any = [r.min_center_x, r.max_center_x, r.min_center_y, r.max_center_y]
            .iter()
            .any(Option::is_some);
        any.then(|| Self {
            min_x: r.min_center_x.unwrap_or(f64::NEG_INFINITY),
            max_x: r.max_center_x.unwrap_or(f64::INFINITY),
            min_y: r.min_center_y.unwrap_or(f64::NEG_INFINITY),
            max_y: r.max_center_y.unwrap_or(f64::INFINITY),
        })
    }

    /// Linear seeds have no center, so they never pass.
    fn passes(&self, center: Option<(f64, f64)>) -> bool {
        // Small slack so a slider stop at e.g. 33% still catches the ⅓ value.
        const EPS: f64 = 1e-6;
        match center {
            Some((x, y)) => {
                x >= self.min_x - EPS
                    && x <= self.max_x + EPS
                    && y >= self.min_y - EPS
                    && y <= self.max_y + EPS
            }
            None => false,
        }
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
    // Structure filters are checked from a cheap pre-layout replay; only the
    // point-level filters (or the "run layout" option) need a full layout.
    let structure_filter = char_filter.filter(|f| f.structure_active());
    let points_filter = char_filter.filter(|f| f.points_active());
    let needs_layout = request.run_layout || points_filter.is_some();
    // A seed is rejected once it has more than `max_points`, so the layout
    // can stop the moment it crosses that line.
    let point_cap = points_filter.and_then(|f| f.max_points);
    let angle_active = angle_filter_active(request.min_angle_deg, request.max_angle_deg);
    let min_angle = request.min_angle_deg;
    let max_angle = request.max_angle_deg;
    let center_bounds = CenterBounds::from_request(&request);
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
    let timed_out = Arc::new(AtomicUsize::new(0));
    let emitter_done = Arc::new(AtomicBool::new(false));
    let on_progress = Arc::new(on_progress);

    // Built before the emitter thread starts, so a failure here can't leave
    // that thread running.
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(workers)
        .thread_name(|i| format!("qql-search-{i}"))
        .build()
        .map_err(|e| anyhow::anyhow!("failed to build thread pool: {e}"))?;

    // Spawn a dedicated emitter so the UI sees regular updates even if the
    // search itself runs in tight, sub-millisecond loops.
    let emitter_handle = {
        let attempts_atomic = attempts_atomic.clone();
        let found_atomic = found_atomic.clone();
        let inflight_attempts = inflight_attempts.clone();
        let inflight_found = inflight_found.clone();
        let timed_out = timed_out.clone();
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
                    timed_out: 0,
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
                        timed_out: timed_out.load(Ordering::Relaxed),
                    });
                }
            })
            .expect("failed to spawn progress emitter")
    };

    // Run the search in a closure so that any early `?` exit still reaches the
    // emitter shutdown below.
    let mut search = || -> anyhow::Result<()> {
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
                            if let Some(bounds) = center_bounds {
                                let center = qql::art::flow_center_frac(seed, render::color_db());
                                if !bounds.passes(center) {
                                    inflight_attempts.fetch_add(1, Ordering::Relaxed);
                                    return None;
                                }
                            }
                            if let Some(filter) = structure_filter {
                                if !filter.matches_structure(&render::structure_only(seed)) {
                                    inflight_attempts.fetch_add(1, Ordering::Relaxed);
                                    return None;
                                }
                            }
                            let stats = if needs_layout {
                                let deadline =
                                    layout_timeout.map(|d| std::time::Instant::now() + d);
                                match render::layout_only_with_limits(
                                    seed,
                                    deadline,
                                    point_cap,
                                    Some(&CANCEL),
                                ) {
                                    Some(s) => Some(s),
                                    None => {
                                        // Timed out, went over the max-points cap,
                                        // or was cancelled — count the attempt and
                                        // move on.
                                        if deadline.is_some_and(|d| std::time::Instant::now() >= d) {
                                            timed_out.fetch_add(1, Ordering::Relaxed);
                                        }
                                        inflight_attempts.fetch_add(1, Ordering::Relaxed);
                                        return None;
                                    }
                                }
                            } else {
                                None
                            };
                            if let (Some(filter), Some(s)) = (points_filter, stats.as_ref()) {
                                if !filter.matches_points(s) {
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
        Ok(())
    };
    let search_result = search();

    // Stop the emitter (also on error) and flush a definitive final state.
    emitter_done.store(true, Ordering::Relaxed);
    let _ = emitter_handle.join();
    search_result?;
    let timed_out = timed_out.load(Ordering::Relaxed);
    on_progress(GenerateProgress {
        attempts,
        found: results.len(),
        max_attempts,
        want,
        timed_out,
    });

    Ok(GenerateOutcome {
        candidates: results,
        attempts,
        max_attempts,
        cancelled,
        timed_out,
    })
}

#[tauri::command]
pub async fn render_seed(
    seed_hex: String,
    width: i32,
    inflate: Option<bool>,
) -> Result<RenderResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let bytes = parse_seed_hex(&seed_hex).map_err(|e| e.to_string())?;
        let w = width.clamp(64, 9600);
        render::render(&bytes, w, inflate.unwrap_or(false)).map_err(|e| format!("{e:#}"))
    })
    .await
    .map_err(|e| format!("worker join error: {e}"))?
}

/// File name for a favorited seed: `0x<lowercase hex>.png`. Built from the
/// parsed seed so save and delete always agree, whatever the input's case,
/// prefix or whitespace.
fn favorite_filename(seed: &[u8; 32]) -> String {
    format!("{}.png", seed_to_hex(seed))
}

/// Render a seed at `width` and write it as `<folder>/0x<seed-hex>.png`.
/// Returns the full path written. Used by the favorite (heart) action.
#[tauri::command]
pub async fn save_seed_png(
    seed_hex: String,
    width: i32,
    folder: String,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let bytes = parse_seed_hex(&seed_hex).map_err(|e| e.to_string())?;
        let w = width.clamp(64, 9600);
        if !std::path::Path::new(&folder).is_dir() {
            return Err(format!(
                "Favorites folder not found: {folder}. Choose the save folder again."
            ));
        }
        let png = render::render_png_bytes(&bytes, w, false).map_err(|e| format!("{e:#}"))?;
        let path = std::path::Path::new(&folder).join(favorite_filename(&bytes));
        std::fs::write(&path, &png)
            .map_err(|e| format!("write failed ({}): {e}", path.display()))?;
        Ok(path.to_string_lossy().into_owned())
    })
    .await
    .map_err(|e| format!("worker join error: {e}"))?
}

/// Seeds that have a saved favorite PNG (`0x<64 hex>.png`) in `folder`, so
/// the hearts can be restored on launch. A missing folder gives an empty list.
#[tauri::command]
pub fn list_favorites(folder: String) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(&folder) else {
        return Vec::new();
    };
    entries
        .filter_map(|e| e.ok())
        .filter_map(|e| e.file_name().into_string().ok())
        .filter_map(|name| {
            let stem = name.strip_suffix(".png")?;
            let bytes = parse_seed_hex(stem).ok()?;
            // Only files in the exact form we write, not any PNG named like hex.
            (favorite_filename(&bytes) == name).then(|| seed_to_hex(&bytes))
        })
        .collect()
}

/// Delete a previously-saved favorite PNG (un-hearting). Missing file is treated
/// as success so the UI stays consistent.
#[tauri::command]
pub fn delete_seed_png(folder: String, seed_hex: String) -> Result<(), String> {
    let bytes = parse_seed_hex(&seed_hex).map_err(|e| e.to_string())?;
    let path = std::path::Path::new(&folder).join(favorite_filename(&bytes));
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("delete failed ({}): {e}", path.display())),
    }
}

/// Render a seed at `width` and write it to an exact destination `path` chosen
/// by the user (the Detail view's "Save As…"). Returns the path written.
#[tauri::command]
pub async fn export_seed_png(
    seed_hex: String,
    width: i32,
    path: String,
    inflate: Option<bool>,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let bytes = parse_seed_hex(&seed_hex).map_err(|e| e.to_string())?;
        let w = width.clamp(64, 9600);
        let png = render::render_png_bytes(&bytes, w, inflate.unwrap_or(false))
            .map_err(|e| format!("{e:#}"))?;
        std::fs::write(&path, &png).map_err(|e| format!("write failed ({path}): {e}"))?;
        Ok(path)
    })
    .await
    .map_err(|e| format!("worker join error: {e}"))?
}

/// Cancel flag for an in-flight animation render (best-effort: stops collecting
/// frames between frames; the underlying draw can't be interrupted mid-frame).
static CANCEL_ANIM: Lazy<AtomicBool> = Lazy::new(|| AtomicBool::new(false));

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AnimationProgress {
    pub done: usize,
    pub total: usize,
    /// "rendering" while frames are produced, "encoding" while writing output.
    pub phase: String,
}

#[tauri::command]
pub fn cancel_animation() {
    CANCEL_ANIM.store(true, Ordering::SeqCst);
}

/// Render an animation and return the frames as PNG data URLs for the in-app
/// player. Width is capped to a preview-friendly size. `num_points` is the
/// already-known layout point count, if the frontend has stats (saves a
/// layout pass that would otherwise run just to size the frame step).
#[tauri::command]
pub async fn animation_preview(
    app: AppHandle,
    seed_hex: String,
    width: i32,
    frames: usize,
    num_points: Option<usize>,
) -> Result<Vec<String>, String> {
    CANCEL_ANIM.store(false, Ordering::SeqCst);
    tauri::async_runtime::spawn_blocking(move || {
        let bytes = parse_seed_hex(&seed_hex).map_err(|e| e.to_string())?;
        let w = width.clamp(64, 1280);
        let app2 = app.clone();
        let pngs = render::render_animation_frames(
            &bytes,
            w,
            frames,
            num_points,
            |done, total| {
                let _ = app2.emit(
                    "animation-progress",
                    AnimationProgress {
                        done,
                        total,
                        phase: "rendering".into(),
                    },
                );
            },
            &CANCEL_ANIM,
        )
        .map_err(|e| format!("{e:#}"))?;
        Ok(pngs.iter().map(|p| render::png_to_data_url(p)).collect())
    })
    .await
    .map_err(|e| format!("worker join error: {e}"))?
}

/// Render an animation and write it out. `format` is "apng" (write APNG to the
/// `dest` file path) or "frames" (write numbered PNGs into the `dest` folder).
/// Returns the path written.
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn export_animation(
    app: AppHandle,
    seed_hex: String,
    width: i32,
    frames: usize,
    format: String,
    delay_ms: u16,
    hold_ms: u16,
    dest: String,
    num_points: Option<usize>,
) -> Result<String, String> {
    CANCEL_ANIM.store(false, Ordering::SeqCst);
    tauri::async_runtime::spawn_blocking(move || {
        let bytes = parse_seed_hex(&seed_hex).map_err(|e| e.to_string())?;
        let w = width.clamp(64, 4800);
        let app2 = app.clone();
        let pngs = render::render_animation_frames(
            &bytes,
            w,
            frames,
            num_points,
            |done, total| {
                let _ = app2.emit(
                    "animation-progress",
                    AnimationProgress {
                        done,
                        total,
                        phase: "rendering".into(),
                    },
                );
            },
            &CANCEL_ANIM,
        )
        .map_err(|e| format!("{e:#}"))?;
        let n = pngs.len();
        let _ = app.emit(
            "animation-progress",
            AnimationProgress {
                done: n,
                total: n,
                phase: "encoding".into(),
            },
        );
        match format.as_str() {
            "apng" => {
                let data =
                    render::encode_apng(&pngs, delay_ms, hold_ms).map_err(|e| format!("{e:#}"))?;
                std::fs::write(&dest, &data).map_err(|e| format!("write failed: {e}"))?;
                Ok(dest)
            }
            "frames" => {
                let dir = std::path::Path::new(&dest);
                render::write_frame_sequence(&pngs, dir).map_err(|e| format!("{e:#}"))?;
                Ok(dir.to_string_lossy().into_owned())
            }
            other => Err(format!("unknown format: {other}")),
        }
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
            min_center_x: None,
            max_center_x: None,
            min_center_y: None,
            max_center_y: None,
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
    fn spiral_center_filter_keeps_only_centers_in_range() {
        let mut choices = TraitChoices::default();
        choices.flow_field = Some("Spiral".into());
        let req = GenerateRequest {
            address: "0x33c9371d25ce44a408f8a6473fbad86bf81e1a17".into(),
            choices,
            mode: GenerateMode::Construct,
            count: 6,
            max_attempts: Some(2000),
            run_layout: false,
            characteristic_filter: None,
            rng_seed: Some(7),
            workers: Some(2),
            min_angle_deg: None,
            max_angle_deg: None,
            min_center_x: Some(2.0 / 6.0),
            max_center_x: Some(4.0 / 6.0),
            min_center_y: Some(2.0 / 6.0),
            max_center_y: Some(4.0 / 6.0),
            layout_timeout_ms: None,
        };
        let outcome = generate_candidates_inner(req, |_| {}).unwrap();
        assert_eq!(outcome.candidates.len(), 6);
        for c in &outcome.candidates {
            let bytes: [u8; 32] = hex::decode(c.seed.trim_start_matches("0x"))
                .unwrap()
                .try_into()
                .unwrap();
            let (x, y) = qql::art::flow_center_frac(&bytes, render::color_db()).unwrap();
            assert!((1.0 / 3.0 - 1e-6..=2.0 / 3.0 + 1e-6).contains(&x), "{} x={x}", c.seed);
            assert!((1.0 / 3.0 - 1e-6..=2.0 / 3.0 + 1e-6).contains(&y), "{} y={y}", c.seed);
        }
    }

    #[test]
    fn structure_only_matches_layout() {
        // The pre-layout replay must report exactly what a full layout does.
        // Seedbox seeds cover all three structures.
        let seeds = [
            "34a2438811b7dd3d14aca7f31542d4666e4c37ea7f18376cd749ffff10a10a97",
            "db0df63435a64132ddf7c626011cb3bb370150cba3467b385abaffff10e2c310",
            "e25ecd59478ce509232e37c2e0810fd4c655b8b4e4fab254db31ffff10e22b37",
            "d90cebceb3c7c43b9266fc3d0c2083bbf942b79e08ccd211f229ffff11610ad3",
            "0b62e5caff1b4f9605e78c3ca8016c0a4b4e23cb764fc23fc4cfffff10818aa5",
            "7e23a0a9fd1105ca4b1dc9b7e0f0aab3b7071caa6f0085961046ffff10a20b12",
            "82d8e20bdc15dcba70bc1bd2024ec42334141a544e45564d098effff14e8c32f",
            "b7ea1f23d9956c5f7335285834aa02f2e663ba94112b2ee0343affff10e2ca0b",
            "b630abd9a5367763b7cba316e870c4a54064cc9fc402a78e685cffff1168c017",
            "e25ecd59478ce509232e37c2e0810fd4c655b8b4dd11b75d7b0dffff10e9d336",
            "5c736d849527bb15fbc0111f4ca13b4cfc1a5c8d720b9127bb76ffff1e8d1d7f",
            "dde2e1aa38442089bdb28be76009f42c65df45db90b8ee473a15ffff10c108aa",
            "acfa3e822961cede6dabbbeef96838fec3ad3d47e0df837855ddffff10e04337",
            "9d53fd5e1eb168e58988a9acbd7d7ae7ebba9910aa58b147b662ffff102a4337",
            "7f7d6bf9256fd67ed85e8fc6805d610de3f002650be19d5bd6e6ffff10e9d357",
            "4dc6750a016db9a96fef40d852e1faee11733db6fb8cd6b2e6c3ffff1ecdcb26",
            "7f4679ed286cb37e5925d6ea4e5f75a1bf39df875773bab24742ffff10c18aae",
            "9380da47618d355a9eec7a2ec37e50c2df9bca6bda64b0ab3c8effff10ea632e",
            "9275776b9ac4c62ce29a3d3b62623c35961afa9e6e21e777c538ffff12e56335",
            "039cd535c7d3bcb04e93ad44491b30185ee0a4c536cd91022f55ffff14a50516",
            "5b33b6f41e87111b7d22685b297f086b5535034dde87421cceecffff10e5032d",
            "0b62e5caff1b4f9605e78c3ca8016c0a4b4e23cbe32a62fef9a0ffff1069c246",
            "44266f38ea9ef4e85a77310518b1cb6d5a56349b34b632fc6ef2ffff1021cb17",
            "7e23a0a9fd1105ca4b1dc9b7e0f0aab3b7071caaf69cffc7d4c4ffff11416c03",
            "e25ecd59478ce509232e37c2e0810fd4c655b8b46998a80e5857ffff11010aa3",
            "941beced3e87a15ba22e1a3705b547f50cfd2eb1f876c962d7c5ffff10c1cb0f",
            "5c736d849527bb15fbc0111f4ca13b4cfc1a5c8dc0261f3c514effff162d0b0a",
            "be957a475844c127ddd207b4ff1f63900fd13e57fd0ef6b57b44ffff10ea02d7",
            "7f7d6bf9256fd67ed85e8fc6805d610de3f0026539a23052b635ffff1002ca0b",
            "d22d6be1929eb090a2aa72243d2c6e4dd325fda287d2871efc57ffff10c162a6",
        ];
        for hex_str in seeds {
            let bytes: [u8; 32] = hex::decode(hex_str).unwrap().try_into().unwrap();
            let quick = render::structure_only(&bytes);
            let full = render::layout_only(&bytes).structure();
            assert_eq!(format!("{quick:?}"), format!("{full:?}"), "seed {hex_str}");
        }
    }

    fn formation_request(filter: CharacteristicFilter, count: usize) -> GenerateRequest {
        let mut choices = TraitChoices::default();
        choices.structure = Some("Formation".into());
        choices.flow_field = Some("RandomLinear".into());
        choices.spacing = Some("Dense".into());
        GenerateRequest {
            address: "0x33c9371d25ce44a408f8a6473fbad86bf81e1a17".into(),
            choices,
            mode: GenerateMode::Construct,
            count,
            max_attempts: Some(1_000_000),
            run_layout: false,
            characteristic_filter: Some(filter),
            rng_seed: Some(3),
            workers: Some(4),
            min_angle_deg: Some(43.0),
            max_angle_deg: Some(47.0),
            min_center_x: None,
            max_center_x: None,
            min_center_y: None,
            max_center_y: None,
            layout_timeout_ms: None,
        }
    }

    #[test]
    fn section_filter_runs_without_layout() {
        let filter = CharacteristicFilter {
            max_horizontal_sections: Some(1),
            max_vertical_sections: Some(1),
            ..Default::default()
        };
        assert!(filter.structure_active() && !filter.points_active());
        let outcome = generate_candidates_inner(formation_request(filter, 20), |_| {}).unwrap();
        assert_eq!(outcome.candidates.len(), 20);
        for c in &outcome.candidates {
            // No layout ran, so no point-level stats were attached.
            assert!(c.stats.is_none());
            let bytes: [u8; 32] = hex::decode(c.seed.trim_start_matches("0x"))
                .unwrap()
                .try_into()
                .unwrap();
            let dims = render::layout_only(&bytes).formation_dims.unwrap();
            assert_eq!((dims.horizontal, dims.vertical), (1, 1), "{}", c.seed);
        }
    }

    #[test]
    fn mixed_structure_and_point_filters_still_apply() {
        let filter = CharacteristicFilter {
            max_horizontal_sections: Some(2),
            min_points: Some(20_000),
            ..Default::default()
        };
        assert!(filter.structure_active() && filter.points_active());
        let outcome = generate_candidates_inner(formation_request(filter, 4), |_| {}).unwrap();
        assert_eq!(outcome.candidates.len(), 4);
        for c in &outcome.candidates {
            let stats = c.stats.as_ref().expect("point filter needs a layout");
            assert!(stats.num_points >= 20_000, "{}", c.seed);
            assert!(stats.formation_dims.unwrap().horizontal <= 2, "{}", c.seed);
        }
    }

    #[test]
    fn point_cap_never_changes_which_seeds_pass() {
        // Stopping at the cap must reject exactly the seeds with more than
        // `cap` points, and leave everyone else's stats untouched.
        let seeds = [
            "09039d8d4698d9274fdb3971296435c2399bb6601387593b4f18ffffbfed0b32",
            "039cd535c7d3bcb04e93ad44491b30185ee0a4c5054a62710a45ffff10a1ca43",
            "e21a1c6f4e15a3609d1cd2a7974ec6fb8d53f5941f16ad8eb151ffff10c1c32b",
            "acfa3e822961cede6dabbbeef96838fec3ad3d47630a0346f8e7ffff11018b03",
            "34df5e2952ded2c258b1e7861f972b4ab4f9056e2c8d50d7490cffff18fe0b6f",
            "9380da47618d355a9eec7a2ec37e50c2df9bca6b5d506b09c786ffff1116632d",
            "54fe3b04afe653ef91dd7e5c1c48d40d903b431a59b48001a29fffff11210aac",
            "4939c898da18b0c1e71dc2a42aa545ad228711b55c626ca33661ffff10e10b57",
            "82d8e20bdc15dcba70bc1bd2024ec42334141a543e4f44b3d858ffff12a56534",
            "37b15e06ad0f4520ccdd8c2557d2a28da0354f7d97070d1f7248ffff10a10a13",
            "9380da47618d355a9eec7a2ec37e50c2df9bca6b9ba3b818ad0fffff10e14a12",
            "d138a9b95e50781e247c446c10e197e9d584e197257835120b04ffff10946b37",
            "3a78551911f2ef8a598053d50939b39fc8ec9390c1d55d2ef110ffff10a24a09",
            "0b62e5caff1b4f9605e78c3ca8016c0a4b4e23cbca3c666062acffff10e95317",
            "34a2438811b7dd3d14aca7f31542d4666e4c37ea4389ca80c09cffff11208d12",
            "d90cebceb3c7c43b9266fc3d0c2083bbf942b79ec55e4027783bffff10e20097",
            "af12a9af19331fa9b89915537ce9d41bce5e4c5d34a31be40b0cffff10e22b37",
            "e25ecd59478ce509232e37c2e0810fd4c655b8b4d94fd53f1013ffff10010aa3",
            "a9022f64e9fd13ffb480f73c84a718bc07043f2a425f4d5fae00ffff10d12515",
            "acfa3e822961cede6dabbbeef96838fec3ad3d4796481a81e138ffff1181ca23",
        ];
        for hex_str in seeds {
            let bytes: [u8; 32] = hex::decode(hex_str).unwrap().try_into().unwrap();
            let full = render::layout_only(&bytes);
            let n = full.num_points;
            for cap in [n.saturating_sub(1), n, n + 1, n / 2] {
                let capped = render::layout_only_with_limits(&bytes, None, Some(cap), None);
                assert_eq!(capped.is_some(), n <= cap, "seed {hex_str} n={n} cap={cap}");
                if let Some(c) = capped {
                    assert_eq!(format!("{c:?}"), format!("{full:?}"), "seed {hex_str}");
                }
            }
        }
    }

    #[test]
    fn background_filter_accepts_any_of_several() {
        let allowed = vec!["Fidenza Light Green".to_string(), "Fidenza Pink".to_string()];
        let filter = CharacteristicFilter {
            background_colors: Some(allowed.clone()),
            ..Default::default()
        };
        assert!(filter.structure_active() && !filter.points_active());
        let mut choices = TraitChoices::default();
        choices.color_palette = Some("Fidenza".into());
        let req = GenerateRequest {
            address: "0x33c9371d25ce44a408f8a6473fbad86bf81e1a17".into(),
            choices,
            mode: GenerateMode::Construct,
            count: 30,
            max_attempts: Some(10_000),
            run_layout: false,
            characteristic_filter: Some(filter),
            rng_seed: Some(5),
            workers: Some(4),
            min_angle_deg: None,
            max_angle_deg: None,
            min_center_x: None,
            max_center_x: None,
            min_center_y: None,
            max_center_y: None,
            layout_timeout_ms: None,
        };
        let outcome = generate_candidates_inner(req, |_| {}).unwrap();
        assert_eq!(outcome.candidates.len(), 30);
        let mut seen = std::collections::HashSet::new();
        for c in &outcome.candidates {
            let bytes: [u8; 32] = hex::decode(c.seed.trim_start_matches("0x"))
                .unwrap()
                .try_into()
                .unwrap();
            let bg = render::structure_only(&bytes).background_color;
            assert!(allowed.contains(&bg), "{} has {bg}", c.seed);
            seen.insert(bg);
        }
        // Both picks should turn up in 30 results (each is ~1/8 of Fidenza).
        assert_eq!(seen.len(), 2, "{seen:?}");
    }

    #[test]
    fn palette_background_odds_sum_to_one() {
        for p in list_palette_colors() {
            let total: f64 = p.backgrounds.iter().map(|b| b.odds).sum();
            assert!((total - 1.0).abs() < 1e-9, "{} sums to {total}", p.palette);
        }
    }

    fn scratch_dir(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("qql-studio-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn unhearting_deletes_the_saved_favorite() {
        let dir = scratch_dir("fav");
        let folder = dir.to_string_lossy().into_owned();
        let seed = "0xF30939AD9A36287DC1CEBD9B784067927E7713429461DC01D066FFFF10810307";
        let saved = tauri::async_runtime::block_on(save_seed_png(seed.into(), 64, folder.clone()))
            .unwrap();
        assert!(saved.ends_with(
            "0xf30939ad9a36287dc1cebd9b784067927e7713429461dc01d066ffff10810307.png"
        ));
        assert!(std::path::Path::new(&saved).exists());
        // Delete with the form the UI passes (lowercase, 0x-prefixed).
        delete_seed_png(folder.clone(), seed.to_lowercase()).unwrap();
        assert!(!std::path::Path::new(&saved).exists());
        // Malformed seeds are refused rather than turned into a path.
        assert!(delete_seed_png(folder, "../../etc/x".into()).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn list_favorites_finds_saved_seeds_only() {
        let dir = scratch_dir("favlist");
        let folder = dir.to_string_lossy().into_owned();
        let a = "0xf30939ad9a36287dc1cebd9b784067927e7713429461dc01d066ffff10810307";
        tauri::async_runtime::block_on(save_seed_png(a.into(), 64, folder.clone())).unwrap();
        // Files that aren't favorites must be ignored.
        std::fs::write(dir.join("notes.png"), b"x").unwrap();
        std::fs::write(dir.join("f30939ad9a36287dc1cebd9b784067927e7713429461dc01d066ffff10810307.png"), b"x").unwrap();
        assert_eq!(list_favorites(folder.clone()), vec![a.to_string()]);
        assert!(list_favorites(dir.join("missing").to_string_lossy().into_owned()).is_empty());
        // Saving into a missing folder explains itself.
        let err = tauri::async_runtime::block_on(save_seed_png(
            a.into(),
            64,
            dir.join("missing").to_string_lossy().into_owned(),
        ))
        .unwrap_err();
        assert!(err.contains("Favorites folder not found"), "{err}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn layout_stops_when_cancelled() {
        let seed: [u8; 32] =
            hex::decode("039cd535c7d3bcb04e93ad44491b30185ee0a4c5054a62710a45ffff10a1ca43")
                .unwrap()
                .try_into()
                .unwrap();
        let cancel = AtomicBool::new(true);
        let t = std::time::Instant::now();
        assert!(render::layout_only_with_limits(&seed, None, None, Some(&cancel)).is_none());
        assert!(t.elapsed() < Duration::from_millis(100), "{:?}", t.elapsed());
        cancel.store(false, Ordering::Relaxed);
        assert!(render::layout_only_with_limits(&seed, None, None, Some(&cancel)).is_some());
    }

    #[test]
    fn timeouts_are_counted_separately() {
        let mut choices = TraitChoices::default();
        choices.spacing = Some("Dense".into());
        let req = GenerateRequest {
            address: "0x33c9371d25ce44a408f8a6473fbad86bf81e1a17".into(),
            choices,
            mode: GenerateMode::Construct,
            count: 4,
            max_attempts: Some(8),
            run_layout: true,
            characteristic_filter: None,
            rng_seed: Some(2),
            workers: Some(4),
            min_angle_deg: None,
            max_angle_deg: None,
            min_center_x: None,
            max_center_x: None,
            min_center_y: None,
            max_center_y: None,
            // 1 ms: every dense layout times out.
            layout_timeout_ms: Some(1),
        };
        let outcome = generate_candidates_inner(req, |_| {}).unwrap();
        assert!(outcome.timed_out > 0, "expected timeouts");
        assert!(outcome.timed_out <= outcome.attempts);
    }

    #[test]
    fn failed_search_stops_its_progress_updates() {
        let calls = Arc::new(AtomicUsize::new(0));
        let counter = calls.clone();
        let mut choices = TraitChoices::default();
        choices.structure = Some("NotAStructure".into());
        let req = GenerateRequest {
            address: "0x33c9371d25ce44a408f8a6473fbad86bf81e1a17".into(),
            choices,
            mode: GenerateMode::Construct,
            count: 4,
            max_attempts: Some(100),
            run_layout: false,
            characteristic_filter: None,
            rng_seed: Some(1),
            workers: Some(2),
            min_angle_deg: None,
            max_angle_deg: None,
            min_center_x: None,
            max_center_x: None,
            min_center_y: None,
            max_center_y: None,
            layout_timeout_ms: None,
        };
        let result = generate_candidates_inner(req, move |_| {
            counter.fetch_add(1, Ordering::Relaxed);
        });
        assert!(result.is_err());
        let after = calls.load(Ordering::Relaxed);
        std::thread::sleep(Duration::from_millis(250));
        assert_eq!(calls.load(Ordering::Relaxed), after, "progress kept firing after the error");
    }

    #[test]
    fn diagonal_passes_an_angle_range_ending_at_45() {
        // 45° plus floating-point noise, as Diagonal seeds compute after a flip.
        assert!(angle_passes(Some(45.000_000_000_000_05), Some(0.0), Some(45.0)));
        assert!(angle_passes(Some(44.999_999_999_999_95), Some(45.0), Some(90.0)));
        assert!(!angle_passes(Some(45.5), Some(0.0), Some(45.0)));
    }

    #[test]
    fn frame_export_clears_stale_frames() {
        let dir = scratch_dir("frames");
        let frames = |n: usize| vec![vec![0u8; 4]; n];
        render::write_frame_sequence(&frames(5), &dir).unwrap();
        std::fs::write(dir.join("notes.txt"), "keep me").unwrap();
        render::write_frame_sequence(&frames(2), &dir).unwrap();
        let mut names: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(names, ["frame_0001.png", "frame_0002.png", "notes.txt"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn orbital_center_filter_picks_a_side() {
        // Lower third on Y (code 1), and just off the left edge on X (code -2).
        let filter = CharacteristicFilter {
            allowed_orbital_center_x_positions: Some(vec![-2]),
            allowed_orbital_center_y_positions: Some(vec![1]),
            ..Default::default()
        };
        assert!(filter.structure_active() && !filter.points_active());
        let mut choices = TraitChoices::default();
        choices.structure = Some("Orbital".into());
        let req = GenerateRequest {
            address: "0x33c9371d25ce44a408f8a6473fbad86bf81e1a17".into(),
            choices,
            mode: GenerateMode::Construct,
            count: 12,
            max_attempts: Some(20_000),
            run_layout: false,
            characteristic_filter: Some(filter),
            rng_seed: Some(11),
            workers: Some(4),
            min_angle_deg: None,
            max_angle_deg: None,
            min_center_x: None,
            max_center_x: None,
            min_center_y: None,
            max_center_y: None,
            layout_timeout_ms: None,
        };
        let outcome = generate_candidates_inner(req, |_| {}).unwrap();
        assert_eq!(outcome.candidates.len(), 12);
        for c in &outcome.candidates {
            let bytes: [u8; 32] = hex::decode(c.seed.trim_start_matches("0x"))
                .unwrap()
                .try_into()
                .unwrap();
            let oi = render::layout_only(&bytes).orbital_info.unwrap();
            assert!((oi.center_x_frac + 0.333).abs() < 1e-6, "{} x={}", c.seed, oi.center_x_frac);
            assert!((oi.center_y_frac - 0.666).abs() < 1e-6, "{} y={}", c.seed, oi.center_y_frac);
            assert_eq!((oi.center_x_position, oi.center_y_position), (-2, 1));
        }
    }

    #[test]
    fn flow_center_frac_matches_layout() {
        // The cheap pre-layout helper must agree with the center the full
        // layout reports, for radial seeds (Some) and linear seeds (None).
        let seeds = [
            "f30939ad9a36287dc1cebd9b784067927e7713429461dc01d066ffff10810307",
            "a9022f64e9fd13ffb480f73c84a718bc07043f2a425f4d5fae00ffff10d12515",
            "9275776b9ac4c62ce29a3d3b62623c35961afa9e6e21e777c538ffff12e56335",
            "db0df63435a64132ddf7c626011cb3bb370150cba3467b385abaffff10e2c310",
            "37b15e06ad0f4520ccdd8c2557d2a28da0354f7d97070d1f7248ffff10a10a13",
            "44266f38ea9ef4e85a77310518b1cb6d5a56349ba9fb12aa9366ffff10a1c943",
        ];
        for hex_str in seeds {
            let bytes: [u8; 32] = hex::decode(hex_str).unwrap().try_into().unwrap();
            let quick = qql::art::flow_center_frac(&bytes, render::color_db());
            let flow = render::layout_only(&bytes).flow;
            let full = flow.center_x_frac.zip(flow.center_y_frac);
            assert_eq!(quick, full, "seed {hex_str}");
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
    fn animation_frames_and_apng() {
        use std::sync::atomic::AtomicBool;
        let seed = hex_literal::hex!(
            "33c9371d25ce44a408f8a6473fbad86bf81e1a17a2e52c90cf66ffff1296712e"
        );
        let cancel = AtomicBool::new(false);
        let frames =
            render::render_animation_frames(&seed, 240, 20, None, |_done, _total| {}, &cancel)
                .unwrap();
        assert!(
            frames.len() > 2,
            "expected several frames, got {}",
            frames.len()
        );
        // Each frame is a valid PNG.
        assert_eq!(&frames[0][0..8], b"\x89PNG\r\n\x1a\n");
        // Asked for 20; the step rounds up, so allow only the extra first and
        // final frames on top.
        assert!(frames.len() <= 22, "asked for 20 frames, got {}", frames.len());
        // The APNG is a valid PNG and carries the animation-control (acTL) chunk.
        let apng = render::encode_apng(&frames, 50, 500).unwrap();
        assert_eq!(&apng[0..8], b"\x89PNG\r\n\x1a\n");
        assert!(
            apng.windows(4).any(|w| w == b"acTL"),
            "APNG missing acTL chunk"
        );
    }

    #[test]
    fn render_returns_png_bytes() {
        let result = render::render(
            &hex_literal::hex!(
                "33c9371d25ce44a408f8a6473fbad86bf81e1a17a2e52c90cf66ffff1296712e"
            ),
            240,
            false,
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


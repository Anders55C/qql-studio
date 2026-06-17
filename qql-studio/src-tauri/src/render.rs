//! Render a QQL seed to PNG bytes, and expose layout-only summary helpers.

use anyhow::{Context, Result};
use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use image::{ImageBuffer, ImageFormat, Rgba};
use once_cell::sync::Lazy;
use qql::{art, color::ColorDb, config::Config};
use serde::Serialize;
use std::io::Cursor;
use std::time::Instant;

static COLOR_DB: Lazy<ColorDb> = Lazy::new(ColorDb::from_bundle);

pub fn color_db() -> &'static ColorDb {
    &COLOR_DB
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct LayoutStats {
    pub num_points: usize,
    pub colors_used: Vec<String>,
    pub ring_counts: Vec<RingCountBucket>,
    pub radius_buckets: RadiusBucketsWire,
    pub quadrants: QuadrantsWire,
    pub background_color: String,
    /// Populated only for Formation pieces.
    pub formation_dims: Option<FormationDimsWire>,
    /// Populated only for Orbital pieces.
    pub orbital_info: Option<OrbitalInfoWire>,
    /// Populated only for Shadows pieces.
    pub shadows_info: Option<ShadowsInfoWire>,
    /// Flow line length picked by the algorithm (500 / 650 / 850).
    pub curve_length: usize,
    /// Per-point probability of splatter satellite spawn. 0.0 = no splatter.
    pub splatter_odds: f64,
}

#[derive(Serialize, Clone, Debug, Copy)]
#[serde(rename_all = "camelCase")]
pub struct FormationDimsWire {
    pub horizontal: usize,
    pub vertical: usize,
    pub total: usize,
    /// Point spacing within each chunk, as a fraction of canvas width.
    pub step_frac: f64,
    /// Per-chunk skip probability (one of 0.0 / 0.1 / 0.2 / 0.5).
    pub skip_odds: f64,
    /// Chunks actually placed after the skip filter.
    pub actual_chunks: usize,
}

impl From<qql::layouts::FormationDims> for FormationDimsWire {
    fn from(d: qql::layouts::FormationDims) -> Self {
        Self {
            horizontal: d.horizontal_sections,
            vertical: d.vertical_sections,
            total: d.horizontal_sections * d.vertical_sections,
            step_frac: d.step_frac,
            skip_odds: d.skip_odds,
            actual_chunks: d.actual_chunks,
        }
    }
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct OrbitalInfoWire {
    pub ring_bands: usize,
    pub min_splits: u32,
    pub max_splits: u32,
    /// Distinct split values that appeared somewhere in the piece (subset of
    /// `{1, 2, 3}`). Order is ascending.
    pub splits_used: Vec<u32>,
    pub center_on_canvas: bool,
    pub center_x_frac: f64,
    pub center_y_frac: f64,
    /// Category indices computed from the raw fractions.
    /// 0 = centered (1/2), 1 = off-center (1/3, 2/3),
    /// 2 = just outside (-1/3, 4/3), 3 = way outside (-1.6, 1.6).
    pub center_x_category: u8,
    pub center_y_category: u8,
    /// Point spacing along each ring, as a fraction of canvas width.
    pub base_step_frac: f64,
    /// Ring band thickness as a fraction of canvas width.
    pub radial_group_step_frac: f64,
    /// Random angular offset in degrees [0, 360). Rarely meaningful.
    pub split_offset_deg: f64,
}

/// Classify a center axis fraction into the four UX categories.
fn classify_center_axis(frac: f64) -> u8 {
    let eps = 0.02;
    if (frac - 0.5).abs() < eps {
        0
    } else if (frac - (1.0 / 3.0)).abs() < eps || (frac - (2.0 / 3.0)).abs() < eps {
        1
    } else if (frac + (1.0 / 3.0)).abs() < eps || (frac - (4.0 / 3.0)).abs() < eps {
        2
    } else {
        3
    }
}

#[derive(Serialize, Clone, Debug, Copy)]
#[serde(rename_all = "camelCase")]
pub struct ShadowsInfoWire {
    pub num_circles_target: u32,
    pub actual_circles: usize,
    pub p_square: f64,
    pub columnar_square: bool,
    pub outward_radial: bool,
}

impl From<qql::layouts::ShadowsInfo> for ShadowsInfoWire {
    fn from(i: qql::layouts::ShadowsInfo) -> Self {
        Self {
            num_circles_target: i.num_circles_target,
            actual_circles: i.actual_circles,
            p_square: i.p_square,
            columnar_square: i.columnar_square,
            outward_radial: i.outward_radial,
        }
    }
}

impl From<qql::layouts::OrbitalInfo> for OrbitalInfoWire {
    fn from(i: qql::layouts::OrbitalInfo) -> Self {
        let splits_used = (0..3)
            .filter_map(|k| if i.splits_used[k] { Some((k as u32) + 1) } else { None })
            .collect();
        let split_offset_deg = i.split_offset_rad.to_degrees();
        Self {
            ring_bands: i.ring_bands,
            min_splits: i.min_splits,
            max_splits: i.max_splits,
            splits_used,
            center_on_canvas: i.center_on_canvas,
            center_x_frac: i.center_x_frac,
            center_y_frac: i.center_y_frac,
            center_x_category: classify_center_axis(i.center_x_frac),
            center_y_category: classify_center_axis(i.center_y_frac),
            base_step_frac: i.base_step_frac,
            radial_group_step_frac: i.radial_group_step_frac,
            split_offset_deg,
        }
    }
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct QuadrantsWire {
    pub top_left: usize,
    pub top_right: usize,
    pub bottom_left: usize,
    pub bottom_right: usize,
}

impl From<art::QuadrantCounts> for QuadrantsWire {
    fn from(q: art::QuadrantCounts) -> Self {
        Self {
            top_left: q.top_left,
            top_right: q.top_right,
            bottom_left: q.bottom_left,
            bottom_right: q.bottom_right,
        }
    }
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct RingCountBucket {
    pub rings: u32,
    pub count: usize,
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct RadiusBucketsWire {
    pub small: usize,
    pub medium: usize,
    pub large: usize,
}

impl From<art::RadiusBuckets> for RadiusBucketsWire {
    fn from(b: art::RadiusBuckets) -> Self {
        Self {
            small: b.small,
            medium: b.medium,
            large: b.large,
        }
    }
}

pub fn layout_only(seed: &[u8; 32]) -> LayoutStats {
    let cfg = Config::default();
    let summary = art::layout(seed, color_db(), &cfg);
    summary_to_stats(&summary)
}

/// Like [`layout_only`] but returns `None` when the layout takes longer than
/// the supplied deadline. Used by the bulk-search loop to skip dense outliers.
pub fn layout_only_with_deadline(
    seed: &[u8; 32],
    deadline: Option<Instant>,
) -> Option<LayoutStats> {
    let cfg = Config::default();
    let summary = art::layout_with_deadline(seed, color_db(), &cfg, deadline)?;
    Some(summary_to_stats(&summary))
}

fn summary_to_stats(summary: &art::LayoutSummary) -> LayoutStats {
    let colors_used = summary
        .colors_used
        .iter()
        .map(|k| {
            color_db()
                .color(k)
                .map(|c| c.name.clone())
                .unwrap_or_else(|| format!("color#{}", k))
        })
        .collect();
    let ring_counts = summary
        .ring_counts_used
        .iter()
        .map(|(rings, count)| RingCountBucket {
            rings: *rings,
            count: *count,
        })
        .collect();
    let background_color = color_db()
        .color(summary.background_color)
        .map(|c| c.name.clone())
        .unwrap_or_else(|| format!("color#{}", summary.background_color));
    LayoutStats {
        num_points: summary.num_points,
        colors_used,
        ring_counts,
        radius_buckets: summary.radius_buckets.into(),
        quadrants: summary.quadrant_counts.into(),
        background_color,
        formation_dims: summary.formation_dims.map(Into::into),
        orbital_info: summary.orbital_info.map(Into::into),
        shadows_info: summary.shadows_info.map(Into::into),
        curve_length: summary.curve_length,
        splatter_odds: summary.splatter_odds,
    }
}

/// Render a seed and return both PNG bytes (as base64 data URL) and stats.
pub fn render(seed: &[u8; 32], width: i32) -> Result<RenderResult> {
    // For small (thumbnail-sized) renders we expect many to happen in parallel
    // from the grid, so we let each render use a single thread and lean on
    // multi-tile parallelism. For large renders (detail view, full-resolution
    // exports) there's typically only one in flight, so we use internal chunking
    // to finish that one faster.
    let chunks = if width >= 1500 { "2x2" } else { "1x1" };
    let cfg = Config {
        chunks: chunks.parse().unwrap(),
        ..Config::default()
    };
    let render_data = art::draw(seed, color_db(), &cfg, width, |_frame| {});
    let png_bytes = drawtarget_to_png_bytes(&render_data.canvas)?;
    let data_url = format!("data:image/png;base64,{}", B64.encode(&png_bytes));
    let colors_used = render_data
        .colors_used
        .iter()
        .map(|k| {
            color_db()
                .color(k)
                .map(|c| c.name.clone())
                .unwrap_or_else(|| format!("color#{}", k))
        })
        .collect();
    let ring_counts = render_data
        .ring_counts_used
        .iter()
        .map(|(rings, count)| RingCountBucket {
            rings: *rings,
            count: *count,
        })
        .collect();
    let background_color = color_db()
        .color(render_data.background_color)
        .map(|c| c.name.clone())
        .unwrap_or_else(|| format!("color#{}", render_data.background_color));
    Ok(RenderResult {
        png_data_url: data_url,
        stats: LayoutStats {
            num_points: render_data.num_points,
            colors_used,
            ring_counts,
            radius_buckets: render_data.radius_buckets.into(),
            quadrants: render_data.quadrant_counts.into(),
            background_color,
            formation_dims: render_data.formation_dims.map(Into::into),
            orbital_info: render_data.orbital_info.map(Into::into),
            shadows_info: render_data.shadows_info.map(Into::into),
            curve_length: render_data.curve_length,
            splatter_odds: render_data.splatter_odds,
        },
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderResult {
    pub png_data_url: String,
    pub stats: LayoutStats,
}

/// Render a seed to raw PNG bytes at the given width. Used by the save-to-disk
/// path, which wants the bytes directly (no base64, no stats).
pub fn render_png_bytes(seed: &[u8; 32], width: i32) -> Result<Vec<u8>> {
    let chunks = if width >= 1500 { "2x2" } else { "1x1" };
    let cfg = Config {
        chunks: chunks.parse().unwrap(),
        ..Config::default()
    };
    let render_data = art::draw(seed, color_db(), &cfg, width, |_frame| {});
    drawtarget_to_png_bytes(&render_data.canvas)
}

/// Convert a raqote DrawTarget into (width, height, RGBA bytes). raqote stores
/// each pixel as u32 in 0xAARRGGBB form; little-endian byte order is [B, G, R, A].
fn drawtarget_to_rgba(dt: &raqote::DrawTarget) -> (u32, u32, Vec<u8>) {
    let (w, h) = (dt.width() as u32, dt.height() as u32);
    let raw = dt.get_data();
    let mut rgba = Vec::<u8>::with_capacity(raw.len() * 4);
    for pixel in raw {
        let [b, g, r, a] = pixel.to_le_bytes();
        rgba.extend_from_slice(&[r, g, b, a]);
    }
    (w, h, rgba)
}

fn drawtarget_to_png_bytes(dt: &raqote::DrawTarget) -> Result<Vec<u8>> {
    let (w, h, rgba) = drawtarget_to_rgba(dt);
    let img: ImageBuffer<Rgba<u8>, Vec<u8>> =
        ImageBuffer::from_vec(w, h, rgba).context("failed to wrap raqote data into image")?;
    let mut buf = Cursor::new(Vec::<u8>::new());
    img.write_to(&mut buf, ImageFormat::Png)
        .context("failed to encode PNG")?;
    Ok(buf.into_inner())
}

/// Render a seed as an "build-in" animation: the piece paints on progressively.
/// Returns one compressed PNG per frame. `target_frames` controls the pace —
/// we derive `Animation::Points { step }` from the piece's point count so the
/// result lands near that many frames. Collecting *compressed* frames keeps
/// memory bounded even at large resolutions.
///
/// `on_progress(done, total_estimate)` is called as each frame is produced, and
/// `cancel` is polled between frames (best-effort: the underlying `draw()` can't
/// be interrupted mid-frame, so this stops collecting rather than instantly
/// freeing the CPU).
pub fn render_animation_frames(
    seed: &[u8; 32],
    width: i32,
    target_frames: usize,
    mut on_progress: impl FnMut(usize, usize),
    cancel: &std::sync::atomic::AtomicBool,
) -> Result<Vec<Vec<u8>>> {
    use std::sync::atomic::Ordering;
    let num_points = layout_only(seed).num_points;
    let frames_wanted = target_frames.clamp(2, 600);
    let step = ((num_points / frames_wanted).max(1)) as u32;
    let total_est = num_points / step.max(1) as usize + 2;
    let chunks = if width >= 1500 { "2x2" } else { "1x1" };
    let cfg = Config {
        animate: qql::config::Animation::Points { step },
        splatter_immediately: true,
        chunks: chunks.parse().unwrap(),
        ..Config::default()
    };
    let mut frames: Vec<Vec<u8>> = Vec::new();
    let mut cancelled = false;
    art::draw(seed, color_db(), &cfg, width, |frame| {
        if cancelled || cancel.load(Ordering::Relaxed) {
            cancelled = true;
            return;
        }
        if let Ok(png) = drawtarget_to_png_bytes(frame.dt) {
            frames.push(png);
            on_progress(frames.len(), total_est);
        }
    });
    if cancelled {
        anyhow::bail!("cancelled");
    }
    Ok(frames)
}

/// Encode a sequence of PNG frames into an animated PNG (APNG). Full color,
/// lossless, loops forever. `delay_ms` is the per-frame delay; the final frame
/// is held for `hold_ms` if that's longer. Frames are decoded one at a time to
/// keep peak memory low.
pub fn encode_apng(frames: &[Vec<u8>], delay_ms: u16, hold_ms: u16) -> Result<Vec<u8>> {
    if frames.is_empty() {
        anyhow::bail!("no frames to encode");
    }
    let first = image::load_from_memory(&frames[0])
        .context("decoding first frame")?
        .to_rgba8();
    let (w, h) = (first.width(), first.height());
    let mut out = Cursor::new(Vec::<u8>::new());
    {
        let mut enc = png::Encoder::new(&mut out, w, h);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.set_animated(frames.len() as u32, 0)
            .context("set_animated")?;
        enc.set_frame_delay(delay_ms, 1000).context("set_frame_delay")?;
        let mut writer = enc.write_header().context("apng write_header")?;
        for (i, f) in frames.iter().enumerate() {
            let rgba = if i == 0 {
                first.as_raw().clone()
            } else {
                image::load_from_memory(f)
                    .with_context(|| format!("decoding frame {i}"))?
                    .to_rgba8()
                    .into_raw()
            };
            if i + 1 == frames.len() && hold_ms > delay_ms {
                writer
                    .set_frame_delay(hold_ms, 1000)
                    .context("final frame hold")?;
            }
            writer
                .write_image_data(&rgba)
                .with_context(|| format!("writing apng frame {i}"))?;
        }
        writer.finish().context("apng finish")?;
    }
    Ok(out.into_inner())
}

/// Wrap raw PNG bytes as a base64 data URL (for sending frames to the webview
/// animation player).
pub fn png_to_data_url(png: &[u8]) -> String {
    format!("data:image/png;base64,{}", B64.encode(png))
}

/// Write a sequence of PNG frames as numbered files into `dir` (created if
/// needed). Returns the directory path.
pub fn write_frame_sequence(frames: &[Vec<u8>], dir: &std::path::Path) -> Result<()> {
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    for (i, f) in frames.iter().enumerate() {
        let name = format!("frame_{:04}.png", i + 1);
        std::fs::write(dir.join(&name), f).with_context(|| format!("writing {name}"))?;
    }
    Ok(())
}

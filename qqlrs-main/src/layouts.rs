use super::art::{h, w};
use super::math::{add_polar_offset, dist, pi};
use super::rand::Rng;
use super::traits::Structure;

#[derive(Debug)]
pub struct StartPointGroups(pub Vec<Vec<(f64, f64)>>);

/// Structure-specific layout information surfaced for downstream filtering /
/// display.
#[derive(Debug, Default, Clone, Copy)]
pub struct StructureInfo {
    pub formation: Option<FormationDims>,
    pub orbital: Option<OrbitalInfo>,
    pub shadows: Option<ShadowsInfo>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FormationDims {
    pub horizontal_sections: usize,
    pub vertical_sections: usize,
    /// Point spacing (`step`) within each chunk, as a fraction of canvas width.
    /// Drawn from `{0.0075, 0.01, 0.02, 0.04, 0.08}`.
    pub step_frac: f64,
    /// Per-chunk probability of being skipped. One of `{0.0, 0.1, 0.2, 0.5}`.
    pub skip_odds: f64,
    /// Number of chunks actually placed (after the skip filter).
    pub actual_chunks: usize,
}

/// Aggregate structural parameters of an Orbital piece. `num_splits` is picked
/// independently per ring band; we surface both its range and the set of
/// distinct values that appeared so callers can do set-based filtering.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OrbitalInfo {
    pub ring_bands: usize,
    pub min_splits: u32,
    pub max_splits: u32,
    /// `splits_used[k]` is true iff at least one band in the piece had
    /// `num_splits = k + 1`. Index 0 → 1-split (continuous ring), index 1 →
    /// 2-split, index 2 → 3-split.
    pub splits_used: [bool; 3],
    /// True iff the orbital center falls inside the visible canvas
    /// (`0 ≤ x ≤ VIRTUAL_W && 0 ≤ y ≤ VIRTUAL_H`).
    pub center_on_canvas: bool,
    /// Center position as a fraction of canvas width/height (e.g. 0.5, 0.333,
    /// 1.333, -1.6). The algorithm picks each axis from a discrete set of 7
    /// values.
    pub center_x_frac: f64,
    pub center_y_frac: f64,
    /// Point spacing along each ring, as a fraction of canvas width.
    /// Drawn from `{0.01, 0.02, 0.04, 0.06, 0.08, 0.16}`.
    pub base_step_frac: f64,
    /// Ring band thickness (radial group step), as a fraction of canvas width.
    /// Drawn from `{0.07, 0.15, 0.30}`.
    pub radial_group_step_frac: f64,
    /// Continuous random angular offset for where wedge splits start.
    /// Range `[0, 2π)`. This is purely a rotation of the whole piece — rarely
    /// meaningful for filtering.
    pub split_offset_rad: f64,
}

/// Aggregate parameters of a Shadows piece. Each piece consists of several
/// non-overlapping circles, each filled with either concentric rings (radial)
/// or a square grid.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShadowsInfo {
    /// Target number of circles the algorithm tried to place. One of
    /// `{5, 7, 10, 20, 30, 60}`.
    pub num_circles_target: u32,
    /// Number of circles actually placed (≤ target — placements that
    /// collided with existing circles are rejected, with a 1000-attempt cap).
    pub actual_circles: usize,
    /// Per-circle probability of being filled with a square grid (vs. radial
    /// rings). One of `{0.0, 0.5, 1.0}`.
    pub p_square: f64,
    /// Square-grid iteration direction. `true` = column-major, `false` =
    /// row-major. Subtle visual effect.
    pub columnar_square: bool,
    /// Radial-fill point ordering. `true` = outside → in, `false` = inside →
    /// out. Affects color layering of concentric rings.
    pub outward_radial: bool,
}

impl StartPointGroups {
    pub fn build(structure: Structure, rng: &mut Rng) -> StartPointGroups {
        Self::build_with_info(structure, rng).0
    }

    /// Like [`build`] but also returns structure-specific parameters that the
    /// underlying layout function picked (e.g. Formation's `(H, V)` grid).
    pub fn build_with_info(
        structure: Structure,
        rng: &mut Rng,
    ) -> (StartPointGroups, StructureInfo) {
        match structure {
            Structure::Orbital => {
                let (groups, info) = orbital(rng);
                (
                    groups,
                    StructureInfo {
                        orbital: Some(info),
                        ..StructureInfo::default()
                    },
                )
            }
            Structure::Shadows => {
                let (groups, info) = shadows(rng);
                (
                    groups,
                    StructureInfo {
                        shadows: Some(info),
                        ..StructureInfo::default()
                    },
                )
            }
            Structure::Formation => {
                let (groups, dims) = formation(rng);
                (
                    groups,
                    StructureInfo {
                        formation: Some(dims),
                        ..StructureInfo::default()
                    },
                )
            }
        }
    }
}

fn orbital(rng: &mut Rng) -> (StartPointGroups, OrbitalInfo) {
    let base_step = *rng.wc(&[
        (w(0.01), 3.0),
        (w(0.02), 2.0),
        (w(0.04), 1.0),
        (w(0.06), 1.0),
        (w(0.08), 1.0),
        (w(0.16), 0.5),
    ]);
    let radial_step = base_step * 0.5;
    let base_step_frac = base_step / w(1.0);

    let radial_group_step = *rng.wc(&[(w(0.07), 0.333), (w(0.15), 0.333), (w(0.3), 0.333)]);
    let radial_group_step_frac = radial_group_step / w(1.0);

    let center_x = *rng.wc(&[
        (w(0.5), 0.3),
        (w(0.333), 0.2),
        (w(0.666), 0.2),
        (w(-0.333), 0.1),
        (w(1.333), 0.1),
        (w(-1.6), 0.05),
        (w(1.6), 0.05),
    ]);
    let center_y = *rng.wc(&[
        (h(0.5), 0.3),
        (h(0.333), 0.2),
        (h(0.666), 0.2),
        (h(-0.333), 0.1),
        (h(1.333), 0.1),
        (h(-1.6), 0.05),
        (h(1.6), 0.05),
    ]);
    let center = (center_x, center_y);
    let center_x_frac = center_x / w(1.0);
    let center_y_frac = center_y / h(1.0);

    let h0 = h(-1.0 / 3.0);
    let h1 = h(4.0 / 3.0);
    let w0 = w(-1.0 / 3.0);
    let w1 = w(4.0 / 3.0);
    let in_bounds = |(x, y)| x > w0 && x < w1 && y > h0 && y < h1;

    let max_radius = w(0.05)
        + (0.0f64)
            .max(dist((center_x, center_y), (0.0, 0.0)))
            .max(dist((center_x, center_y), (w(1.0), 0.0)))
            .max(dist((center_x, center_y), (w(1.0), h(1.0))))
            .max(dist((center_x, center_y), (0.0, h(1.0))));
    let split_offset = rng.uniform(0.0, pi(2.0));

    let mut groups = Vec::new();
    let mut group_radius = w(0.001);
    let mut ring_bands: usize = 0;
    let mut min_splits: u32 = u32::MAX;
    let mut max_splits: u32 = 0;
    let mut splits_used: [bool; 3] = [false; 3];
    while group_radius < max_radius {
        let num_splits = *rng.choice(&[1, 2, 3]);
        ring_bands += 1;
        let splits_u = num_splits as u32;
        if splits_u < min_splits {
            min_splits = splits_u;
        }
        if splits_u > max_splits {
            max_splits = splits_u;
        }
        if (1..=3).contains(&splits_u) {
            splits_used[(splits_u - 1) as usize] = true;
        }
        let split_len = pi(2.0) / num_splits as f64;

        let mut theta = split_offset;
        while theta < split_offset + pi(2.0) {
            let mut group = Vec::new();
            let mut radius = group_radius;
            while radius < group_radius + radial_group_step {
                let circumference = radius * pi(2.0);
                let steps_wanted = circumference / base_step;
                let theta_step = f64::max(pi(0.005), pi(2.0) / steps_wanted);
                let mut inner_theta = theta;
                while inner_theta < theta + split_len {
                    let point = add_polar_offset(center, inner_theta, radius);
                    if in_bounds(point) {
                        group.push(point);
                    }
                    inner_theta += theta_step;
                }
                radius += radial_step;
            }
            groups.push(group);
            theta += split_len;
        }
        group_radius += radial_group_step;
    }
    let center_on_canvas =
        center_x >= 0.0 && center_x <= w(1.0) && center_y >= 0.0 && center_y <= h(1.0);
    // If we somehow exited the loop with no bands recorded, clamp min_splits
    // to a safe value so downstream filters don't see u32::MAX.
    if min_splits == u32::MAX {
        min_splits = 0;
    }
    let info = OrbitalInfo {
        ring_bands,
        min_splits,
        max_splits,
        splits_used,
        center_on_canvas,
        center_x_frac,
        center_y_frac,
        base_step_frac,
        radial_group_step_frac,
        split_offset_rad: split_offset,
    };
    (StartPointGroups(groups), info)
}

fn shadows(rng: &mut Rng) -> (StartPointGroups, ShadowsInfo) {
    let num_circles = *rng.choice(&[5, 7, 10, 20, 30, 60]);
    let num_circles_target = num_circles as u32;
    struct Circle {
        center: (f64, f64),
        radius: f64,
    }

    let p_square = *rng.choice(&[0.0, 0.5, 1.0]);
    let columnar_square = rng.odds(0.5);
    let outward_radial = rng.odds(0.5);

    let collides = |c1: &Circle, c2: &Circle| {
        let d = dist(c1.center, c2.center);
        d < c1.radius + c2.radius
    };

    let radial_fill = |c: Circle, rng: &mut Rng| -> Vec<(f64, f64)> {
        let radius_step = w(0.02);
        let circumference_step = w(0.01);
        let mut radius = c.radius;
        let mut group = Vec::new();
        while radius > 0.0 {
            let num_steps = (radius * pi(2.0)) / circumference_step;
            let theta_step = pi(2.0) / num_steps;
            let mut theta = 0.0;
            while theta < pi(2.01) {
                group.push(add_polar_offset(c.center, theta, radius));
                theta += theta_step;
            }
            radius -= radius_step;
        }
        if outward_radial {
            group.reverse();
        }
        if rng.odds(0.05) {
            group = rng.shuffle(group);
        }
        group
    };

    let square_fill = |c: Circle, rng: &mut Rng| -> Vec<(f64, f64)> {
        let step = *rng.wc(&[
            (w(0.0075), 0.37),
            (w(0.01), 0.35),
            (w(0.02), 0.25),
            (w(0.04), 0.02),
            (w(0.08), 0.01),
        ]);
        let radius = c.radius;
        let r2 = radius * radius;
        let mut group = Vec::new();
        let mut offset1 = -radius;
        while offset1 < radius {
            let mut offset2 = -radius;
            while offset2 < radius {
                let (x, y) = if columnar_square {
                    (c.center.0 + offset1, c.center.1 + offset2)
                } else {
                    (c.center.0 + offset2, c.center.1 + offset1)
                };
                let dx = c.center.0 - x;
                let dy = c.center.1 - y;
                if dx * dx + dy * dy < r2 {
                    group.push((x, y));
                }
                offset2 += step;
            }
            offset1 += step;
        }
        group
    };

    let fill = |c: Circle, rng: &mut Rng| {
        if rng.odds(p_square) {
            square_fill(c, rng)
        } else {
            radial_fill(c, rng)
        }
    };

    let mut iter = 0;
    let mut circles = Vec::with_capacity(num_circles);
    while circles.len() < num_circles && iter < 1000 {
        iter += 1;
        let c = Circle {
            center: (rng.uniform(w(0.0), w(1.0)), rng.uniform(h(0.0), h(1.0))),
            radius: rng.uniform(w(0.05), w(0.5)),
        };
        if circles.iter().all(|c2| !collides(&c, c2)) {
            circles.push(c);
        }
    }
    let actual_circles = circles.len();
    let groups: Vec<Vec<(f64, f64)>> = circles.into_iter().map(|c| fill(c, rng)).collect();
    let info = ShadowsInfo {
        num_circles_target,
        actual_circles,
        p_square,
        columnar_square,
        outward_radial,
    };
    (StartPointGroups(groups), info)
}

fn formation(rng: &mut Rng) -> (StartPointGroups, FormationDims) {
    let step = *rng.wc(&[
        (w(0.0075), 0.37),
        (w(0.01), 0.35),
        (w(0.02), 0.25),
        (w(0.04), 0.02),
        (w(0.08), 0.01),
    ]);

    let num_horizontal_steps = *rng.wc(&[
        (1, 0.7),
        (2, 0.35),
        (3, 0.25),
        (4, 0.1),
        (5, 0.05),
        (7, 0.05),
    ]);
    let num_vertical_steps = *rng.wc(&[
        (1, 0.4),
        (2, 0.35),
        (3, 0.25),
        (4, 0.1),
        (5, 0.05),
        (7, 0.05),
    ]);
    let step_frac = step / w(1.0);

    let horizontal_step_len = w(1.2) / num_horizontal_steps as f64;
    let vertical_step_len = h(1.2) / num_vertical_steps as f64;

    let skip_odds = *rng.wc(&[(0.0, 0.5), (0.1, 0.3), (0.2, 0.15), (0.5, 0.05)]);

    let mut starting_chunks = Vec::with_capacity(num_horizontal_steps * num_vertical_steps);
    {
        let mut x = w(-0.1);
        while x < w(1.1) {
            let mut y = h(-0.1);
            while y < h(1.1) {
                starting_chunks.push((x, y));
                y += vertical_step_len;
            }
            x += horizontal_step_len;
        }
    }
    let starting_chunks = rng.shuffle(starting_chunks);
    let starting_chunks = starting_chunks
        .into_iter()
        .enumerate()
        .filter(|&(i, _pt)| i == 0 || !rng.odds(skip_odds))
        .map(|(_i, pt)| pt);

    let groups: Vec<Vec<(f64, f64)>> = starting_chunks
        .map(|(x_start, y_start)| {
            let mut group = Vec::new();
            let mut y = y_start;
            while y < y_start + vertical_step_len {
                let mut x = x_start;
                while x < x_start + horizontal_step_len {
                    group.push((x, y));
                    x += step;
                }
                y += step;
            }
            group
        })
        .collect();

    let actual_chunks = groups.len();
    let formation_dims = FormationDims {
        horizontal_sections: num_horizontal_steps,
        vertical_sections: num_vertical_steps,
        step_frac,
        skip_odds,
        actual_chunks,
    };

    (StartPointGroups(groups), formation_dims)
}

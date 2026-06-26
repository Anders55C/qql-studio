//! Print the decoded categorical traits for one or more seeds.
//! Usage: cargo run --release --example decode -- <seed_hex>...

use qql::art::{layout, Direction, FlowFieldSpec, Rotation};
use qql::color::ColorDb;
use qql::config::Config;
use qql::rand::Rng;
use qql::traits::Traits;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!("Usage: decode <seed_hex>...");
        std::process::exit(1);
    }
    let color_db = ColorDb::from_bundle();
    let cfg = Config::default();
    for arg in args {
        let s = arg.trim().trim_start_matches("0x");
        let bytes = match hex::decode(s) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("Invalid hex for {}: {}", arg, e);
                continue;
            }
        };
        let seed: [u8; 32] = match bytes.as_slice().try_into() {
            Ok(s) => s,
            Err(_) => {
                eprintln!("Seed {} is not 32 bytes", arg);
                continue;
            }
        };
        let traits = Traits::from_seed(&seed);
        println!("seed       0x{}", hex::encode(seed));
        println!("  flow field      {:?}", traits.flow_field);
        println!("  turbulence      {:?}", traits.turbulence);
        println!("  margin          {:?}", traits.margin);
        println!("  color variety   {:?}", traits.color_variety);
        println!("  color mode      {:?}", traits.color_mode);
        println!("  structure       {:?}", traits.structure);
        println!(
            "  bullseye        1={} 3={} 7={}",
            traits.bullseye_rings.one, traits.bullseye_rings.three, traits.bullseye_rings.seven
        );
        println!("  ring thickness  {:?}", traits.ring_thickness);
        println!("  ring size       {:?}", traits.ring_size);
        println!("  size variety    {:?}", traits.size_variety);
        println!("  color palette   {:?}", traits.color_palette);
        println!("  spacing         {:?}", traits.spacing);
        println!("  version         {:?}", traits.version);
        // Run layout so we can see emergent properties (quadrant density).
        let summary = layout(&seed, &color_db, &cfg);
        let q = summary.quadrant_counts;
        let total = (q.top_left + q.top_right + q.bottom_left + q.bottom_right) as f64;
        let pct = |v: usize| if total > 0.0 { 100.0 * v as f64 / total } else { 0.0 };
        println!(
            "  quadrants       TL {:>4} ({:>5.1}%)   TR {:>4} ({:>5.1}%)",
            q.top_left,
            pct(q.top_left),
            q.top_right,
            pct(q.top_right)
        );
        println!(
            "                  BL {:>4} ({:>5.1}%)   BR {:>4} ({:>5.1}%)",
            q.bottom_left,
            pct(q.bottom_left),
            q.bottom_right,
            pct(q.bottom_right)
        );
        let rb = summary.radius_buckets;
        println!(
            "  radii           small {} · medium {} · large {}   (large = >3% of canvas)",
            rb.small, rb.medium, rb.large
        );
        if let Some(fd) = summary.formation_dims {
            println!(
                "  sections        {} × {}   ({} total)",
                fd.horizontal_sections,
                fd.vertical_sections,
                fd.horizontal_sections * fd.vertical_sections
            );
            println!(
                "                  point spacing {:.2}% · skip odds {:.1} · actual chunks {}",
                fd.step_frac * 100.0,
                fd.skip_odds,
                fd.actual_chunks
            );
        }
        if let Some(oi) = summary.orbital_info {
            let used: Vec<String> = (0..3)
                .filter_map(|k| {
                    if oi.splits_used[k] {
                        Some((k + 1).to_string())
                    } else {
                        None
                    }
                })
                .collect();
            println!(
                "  orbital         {} ring bands · splits used {} · center ({:.3}, {:.3}) {}",
                oi.ring_bands,
                if used.is_empty() {
                    "none".into()
                } else {
                    used.join(",")
                },
                oi.center_x_frac,
                oi.center_y_frac,
                if oi.center_on_canvas {
                    "[on]"
                } else {
                    "[off]"
                }
            );
            println!(
                "                  spacing {:.0}% · band {:.0}% · rotation {:.1}°",
                oi.base_step_frac * 100.0,
                oi.radial_group_step_frac * 100.0,
                oi.split_offset_rad.to_degrees(),
            );
        }
        if let Some(si) = summary.shadows_info {
            let style = if si.p_square <= 0.1 {
                "all radial"
            } else if si.p_square >= 0.9 {
                "all square"
            } else {
                "mixed"
            };
            println!(
                "  shadows         {} / {} circles · fill {} · sq {} · radial {}",
                si.actual_circles,
                si.num_circles_target,
                style,
                if si.columnar_square { "col" } else { "row" },
                if si.outward_radial { "out→in" } else { "in→out" }
            );
        }
        println!("  curve length    {}", summary.curve_length);
        let s = summary.splatter_odds;
        let level = if s <= 0.0 {
            "none"
        } else if s <= 0.005 {
            "light"
        } else if s < 0.08 {
            "moderate"
        } else if s < 0.5 {
            "heavy"
        } else {
            "extreme"
        };
        println!("  splatter        {} (odds {:.3})", level, s);

        // Reconstruct the flow-field spec so we can show the actual angle.
        let mut rng = Rng::from_seed(&seed[..]);
        let spec = FlowFieldSpec::from_traits(&traits, &mut rng);
        match spec {
            FlowFieldSpec::Linear { default_theta } => {
                let deg = default_theta.to_degrees();
                println!(
                    "  flow angle      {:.1}° (linear; 0°=→, 90°=↓, 180°=←, 270°=↑)",
                    deg
                );
            }
            FlowFieldSpec::Radial {
                circularity,
                direction,
                rotation,
                default_theta,
            } => {
                println!(
                    "  flow            radial · circularity={:.2} · direction={} · rotation={} · θ={:.1}°",
                    circularity,
                    match direction { Direction::In => "in", Direction::Out => "out" },
                    match rotation { Rotation::Ccw => "ccw", Rotation::Cw => "cw" },
                    default_theta.to_degrees(),
                );
            }
        }
        println!();
    }
}

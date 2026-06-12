//! Statistical experiment: given fixed traits (size_variety, ring_size), how
//! often do random seeds land on a target point count?
//!
//! Usage:
//!   cargo run --release --example point_count_search -- <attempts> <target> <ring_size>
//!   where <ring_size> is one of Small | Medium | Large
//!
//! Prints histogram around the target and the exact-match rate.

use qql::art::{layout, LayoutSummary};
use qql::color::ColorDb;
use qql::config::Config;
use std::collections::BTreeMap;

fn main() {
    let mut args = std::env::args().skip(1);
    let attempts: usize = args
        .next()
        .and_then(|s| s.parse().ok())
        .unwrap_or(5000);
    let target: usize = args.next().and_then(|s| s.parse().ok()).unwrap_or(32);
    let ring_size = args.next().unwrap_or_else(|| "Medium".into());

    let color_db = ColorDb::from_bundle();
    let cfg = Config::default();

    // Encode our fixed traits into the bottom 4 bytes of every seed. The same
    // bit layout used by Traits::from_seed.
    //
    //   FlowField       : 3 bits  (we leave random)
    //   Turbulence      : 2 bits  (random)
    //   Margin          : 2 bits  (random)
    //   ColorVariety    : 2 bits  (random)
    //   ColorMode       : 2 bits  (random)
    //   Structure       : 2 bits  (random)
    //   bullseye 1      : 1 bit
    //   bullseye 3      : 1 bit
    //   bullseye 7      : 1 bit
    //   RingThickness   : 2 bits  (random)
    //   RingSize        : 2 bits  ← FIXED
    //   SizeVariety     : 2 bits  ← FIXED to Constant (index 0)
    //   ColorPalette    : 3 bits  (random)
    //   Spacing         : 2 bits  (random)
    //   version V1      : top nibble of byte 28
    //
    // For our purposes we mask off the bits we want fixed and OR them in.

    // SizeVariety options: [Constant=0, Variable=1, Wild=2] → 2 bits, want 0.
    // RingSize options: [Small=0, Medium=1, Large=2] → 2 bits.
    let ring_size_idx: u32 = match ring_size.as_str() {
        "Small" => 0,
        "Medium" => 1,
        "Large" => 2,
        other => {
            eprintln!("Unknown ring size: {} (use Small|Medium|Large)", other);
            std::process::exit(1);
        }
    };

    // Bit positions (matching pluck order):
    let mut bit_pos: u32 = 0;
    bit_pos += 3; // FlowField
    bit_pos += 2; // Turbulence
    bit_pos += 2; // Margin
    bit_pos += 2; // ColorVariety
    bit_pos += 2; // ColorMode
    bit_pos += 2; // Structure
    bit_pos += 1; // bullseye 1
    bit_pos += 1; // bullseye 3
    bit_pos += 1; // bullseye 7
    bit_pos += 2; // RingThickness
    let ring_size_bits_pos = bit_pos;
    bit_pos += 2; // RingSize
    let size_variety_bits_pos = bit_pos;
    bit_pos += 2; // SizeVariety

    let ring_size_mask: u32 = 0b11 << ring_size_bits_pos;
    let size_variety_mask: u32 = 0b11 << size_variety_bits_pos;

    let mut histogram: BTreeMap<usize, usize> = BTreeMap::new();
    let mut exact_matches: Vec<[u8; 32]> = Vec::new();
    let mut sum_points: u64 = 0;
    let mut min_seen = usize::MAX;
    let mut max_seen = 0;

    use rand::{Rng, RngCore, SeedableRng};
    let mut rng = rand::rngs::StdRng::from_entropy();
    let address: [u8; 20] = {
        let mut a = [0u8; 20];
        rng.fill_bytes(&mut a);
        a
    };

    eprintln!(
        "Running {} attempts: SizeVariety=Constant, RingSize={}, target={}",
        attempts, ring_size, target
    );

    for attempt in 0..attempts {
        let mut seed = [0u8; 32];
        seed[0..20].copy_from_slice(&address);
        rng.fill_bytes(&mut seed[20..26]);
        seed[26] = 0xff;
        seed[27] = 0xff;
        let mut word: u32 = rng.gen();
        // Force traits: SizeVariety=Constant (idx 0), RingSize=<idx>.
        word &= !size_variety_mask;
        word &= !ring_size_mask;
        word |= ring_size_idx << ring_size_bits_pos;
        // Force version V1 (high nibble of byte 28 = 1).
        word = (word & 0x0fff_ffff) | (1u32 << 28);
        seed[28..32].copy_from_slice(&word.to_be_bytes());

        let summary: LayoutSummary = layout(&seed, &color_db, &cfg);
        let n = summary.num_points;

        *histogram.entry(n).or_insert(0) += 1;
        sum_points += n as u64;
        min_seen = min_seen.min(n);
        max_seen = max_seen.max(n);
        if n == target {
            exact_matches.push(seed);
        }

        if attempt % 200 == 0 && attempt > 0 {
            eprintln!(
                "  {}: exact-matches so far = {} ({:.2}%)",
                attempt,
                exact_matches.len(),
                100.0 * exact_matches.len() as f64 / attempt as f64
            );
        }
    }

    println!();
    println!("RingSize = {}", ring_size);
    println!("Attempts: {}", attempts);
    println!(
        "Mean num_points: {:.1} (min {}, max {})",
        sum_points as f64 / attempts as f64,
        min_seen,
        max_seen
    );
    println!(
        "Exact matches at {} points: {} ({:.3}%)",
        target,
        exact_matches.len(),
        100.0 * exact_matches.len() as f64 / attempts as f64
    );

    println!();
    println!("Histogram (sparse / near target):");
    let near: Vec<(&usize, &usize)> = histogram
        .iter()
        .filter(|(k, _)| **k <= 200)
        .collect();
    for (n, count) in near {
        let bar = "#".repeat((*count).min(60));
        println!("  {:>5} pts: {:>5}  {}", n, count, bar);
    }

    if !exact_matches.is_empty() {
        println!();
        println!("Sample exact-match seeds:");
        for seed in exact_matches.iter().take(5) {
            println!("  0x{}", hex::encode(seed));
        }
    }
}

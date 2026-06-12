//! Seed encoding/decoding and candidate generation.
//!
//! QQL seed layout (32 bytes):
//!   [0..20)  address              — minter's ETH address
//!   [20..26) variation nonce      — affects layout RNG only
//!   [26..28) version sentinel     — must be 0xff 0xff
//!   [28] hi   version (0 or 1)
//!   [28] lo + [29..32)  27 bits of categorical-trait selections
//!
//! `Traits::from_seed` reads bytes [28..32) as a big-endian u32 and `pluck`s
//! bit-fields from the low end. We invert that here.

use anyhow::{anyhow, bail, Context, Result};
use qql::traits as q;
use rand::{Rng, RngCore};
use serde::{Deserialize, Serialize};

/// User-facing trait choice for a single categorical trait.
/// `None` means "any" — the generator will pick freely.
pub type Choice<T> = Option<T>;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TraitChoices {
    pub flow_field: Choice<String>,
    pub turbulence: Choice<String>,
    pub margin: Choice<String>,
    pub color_variety: Choice<String>,
    pub color_mode: Choice<String>,
    pub structure: Choice<String>,
    pub bullseye1: Choice<bool>,
    pub bullseye2: Choice<bool>,
    pub bullseye3: Choice<bool>,
    pub bullseye7: Choice<bool>,
    pub ring_thickness: Choice<String>,
    pub ring_size: Choice<String>,
    pub size_variety: Choice<String>,
    pub color_palette: Choice<String>,
    pub spacing: Choice<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TraitsWire {
    pub flow_field: String,
    pub turbulence: String,
    pub margin: String,
    pub color_variety: String,
    pub color_mode: String,
    pub structure: String,
    pub bullseye1: bool,
    pub bullseye2: bool,
    pub bullseye3: bool,
    pub bullseye7: bool,
    pub ring_thickness: String,
    pub ring_size: String,
    pub size_variety: String,
    pub color_palette: String,
    pub spacing: String,
    pub version: String,
}

impl From<&q::Traits> for TraitsWire {
    fn from(t: &q::Traits) -> Self {
        Self {
            flow_field: format!("{:?}", t.flow_field),
            turbulence: format!("{:?}", t.turbulence),
            margin: format!("{:?}", t.margin),
            color_variety: format!("{:?}", t.color_variety),
            color_mode: format!("{:?}", t.color_mode),
            structure: format!("{:?}", t.structure),
            bullseye1: t.bullseye_rings.one,
            // "Bullseye 2" is the algorithm's fallback when all three of 1/3/7
            // are disabled — see qql::art around `potential_ring_counts`. We
            // surface it here as a derived boolean for filtering/display.
            bullseye2: !t.bullseye_rings.one
                && !t.bullseye_rings.three
                && !t.bullseye_rings.seven,
            bullseye3: t.bullseye_rings.three,
            bullseye7: t.bullseye_rings.seven,
            ring_thickness: format!("{:?}", t.ring_thickness),
            ring_size: format!("{:?}", t.ring_size),
            size_variety: format!("{:?}", t.size_variety),
            color_palette: format!("{:?}", t.color_palette),
            spacing: format!("{:?}", t.spacing),
            version: format!("{:?}", t.version),
        }
    }
}

// ---------------------------------------------------------------------------
// String <-> enum parsing for each trait

macro_rules! parse_enum {
    ($s:expr, $ty:ty, $($name:literal => $variant:expr),* $(,)?) => {
        match $s {
            $($name => Ok::<$ty, anyhow::Error>($variant),)*
            other => Err(anyhow!(concat!("invalid ", stringify!($ty), ": {:?}"), other)),
        }
    };
}

pub fn parse_flow_field(s: &str) -> Result<q::FlowField> {
    parse_enum!(s, q::FlowField,
        "Horizontal" => q::FlowField::Horizontal,
        "Diagonal" => q::FlowField::Diagonal,
        "Vertical" => q::FlowField::Vertical,
        "RandomLinear" => q::FlowField::RandomLinear,
        "Explosive" => q::FlowField::Explosive,
        "Spiral" => q::FlowField::Spiral,
        "Circular" => q::FlowField::Circular,
        "RandomRadial" => q::FlowField::RandomRadial,
    )
}
pub fn parse_turbulence(s: &str) -> Result<q::Turbulence> {
    parse_enum!(s, q::Turbulence,
        "None" => q::Turbulence::None,
        "Low" => q::Turbulence::Low,
        "High" => q::Turbulence::High,
    )
}
pub fn parse_margin(s: &str) -> Result<q::Margin> {
    parse_enum!(s, q::Margin,
        "None" => q::Margin::None,
        "Crisp" => q::Margin::Crisp,
        "Wide" => q::Margin::Wide,
    )
}
pub fn parse_color_variety(s: &str) -> Result<q::ColorVariety> {
    parse_enum!(s, q::ColorVariety,
        "Low" => q::ColorVariety::Low,
        "Medium" => q::ColorVariety::Medium,
        "High" => q::ColorVariety::High,
    )
}
pub fn parse_color_mode(s: &str) -> Result<q::ColorMode> {
    parse_enum!(s, q::ColorMode,
        "Simple" => q::ColorMode::Simple,
        "Stacked" => q::ColorMode::Stacked,
        "Zebra" => q::ColorMode::Zebra,
    )
}
pub fn parse_structure(s: &str) -> Result<q::Structure> {
    parse_enum!(s, q::Structure,
        "Orbital" => q::Structure::Orbital,
        "Formation" => q::Structure::Formation,
        "Shadows" => q::Structure::Shadows,
    )
}
pub fn parse_ring_thickness(s: &str) -> Result<q::RingThickness> {
    parse_enum!(s, q::RingThickness,
        "Thin" => q::RingThickness::Thin,
        "Thick" => q::RingThickness::Thick,
        "Mixed" => q::RingThickness::Mixed,
    )
}
pub fn parse_ring_size(s: &str) -> Result<q::RingSize> {
    parse_enum!(s, q::RingSize,
        "Small" => q::RingSize::Small,
        "Medium" => q::RingSize::Medium,
        "Large" => q::RingSize::Large,
    )
}
pub fn parse_size_variety(s: &str) -> Result<q::SizeVariety> {
    parse_enum!(s, q::SizeVariety,
        "Constant" => q::SizeVariety::Constant,
        "Variable" => q::SizeVariety::Variable,
        "Wild" => q::SizeVariety::Wild,
    )
}
pub fn parse_color_palette(s: &str) -> Result<q::ColorPalette> {
    parse_enum!(s, q::ColorPalette,
        "Austin" => q::ColorPalette::Austin,
        "Berlin" => q::ColorPalette::Berlin,
        "Edinburgh" => q::ColorPalette::Edinburgh,
        "Fidenza" => q::ColorPalette::Fidenza,
        "Miami" => q::ColorPalette::Miami,
        "Seattle" => q::ColorPalette::Seattle,
        "Seoul" => q::ColorPalette::Seoul,
    )
}
pub fn parse_spacing(s: &str) -> Result<q::Spacing> {
    parse_enum!(s, q::Spacing,
        "Dense" => q::Spacing::Dense,
        "Medium" => q::Spacing::Medium,
        "Sparse" => q::Spacing::Sparse,
    )
}

// ---------------------------------------------------------------------------
// Bit packing — mirrors `Traits::from_seed` exactly

fn num_bits<T>(options: &[(T, u32)]) -> u32 {
    let n = options.len();
    if n <= 1 {
        return 0;
    }
    n.next_power_of_two().ilog2()
}

fn option_index<T: Copy + PartialEq>(value: T, options: &[(T, u32)]) -> Result<u32> {
    options
        .iter()
        .position(|(o, _)| *o == value)
        .map(|i| i as u32)
        .ok_or_else(|| anyhow!("value not in options"))
}

/// Pack 13 trait choices + version into the low 27+ bits of a u32.
///
/// Each `pluck` call reads `seed & mask`, then `seed >>= num_bits`. So the FIRST
/// trait (FlowField) occupies the LOW bits of the u32. Version sits in the high
/// nibble of byte 28 (i.e. bits 28-31 of the u32).
fn encode_trait_word(choices: &ResolvedChoices) -> Result<u32> {
    let mut word: u32 = 0;
    let mut pos: u32 = 0;

    macro_rules! push {
        ($value:expr, $options:expr) => {{
            let opts = $options;
            let bits = num_bits(opts);
            let idx = option_index($value, opts)?;
            word |= idx << pos;
            pos += bits;
        }};
    }

    push!(choices.flow_field, q::FlowField::options());
    push!(choices.turbulence, q::Turbulence::options());
    push!(choices.margin, q::Margin::options());
    push!(choices.color_variety, q::ColorVariety::options());
    push!(choices.color_mode, q::ColorMode::options());
    push!(choices.structure, q::Structure::options());
    let bool_opts: &[(bool, u32)] = &[(true, 1), (false, 1)];
    let bull_idx = |b| if b { 0u32 } else { 1u32 };
    word |= bull_idx(choices.bullseye1) << pos;
    pos += num_bits(bool_opts);
    word |= bull_idx(choices.bullseye3) << pos;
    pos += num_bits(bool_opts);
    word |= bull_idx(choices.bullseye7) << pos;
    pos += num_bits(bool_opts);
    push!(choices.ring_thickness, q::RingThickness::options());
    push!(choices.ring_size, q::RingSize::options());
    push!(choices.size_variety, q::SizeVariety::options());
    push!(choices.color_palette, q::ColorPalette::options());
    push!(choices.spacing, q::Spacing::options());

    // Version V1 lives in the high nibble of byte 28, i.e. bits 28..32 of the word.
    // (`Traits::get_version` reads `raw_seed[28] >> 4`, where byte 28 is the high
    // byte of the big-endian u32.)
    let _ = pos;
    word |= 1u32 << 28;

    Ok(word)
}

/// Fully-resolved trait choices (no `Any`s left).
#[derive(Debug, Clone, Copy)]
pub struct ResolvedChoices {
    pub flow_field: q::FlowField,
    pub turbulence: q::Turbulence,
    pub margin: q::Margin,
    pub color_variety: q::ColorVariety,
    pub color_mode: q::ColorMode,
    pub structure: q::Structure,
    pub bullseye1: bool,
    pub bullseye3: bool,
    pub bullseye7: bool,
    pub ring_thickness: q::RingThickness,
    pub ring_size: q::RingSize,
    pub size_variety: q::SizeVariety,
    pub color_palette: q::ColorPalette,
    pub spacing: q::Spacing,
}

fn pick_weighted<T: Copy, R: Rng + ?Sized>(rng: &mut R, options: &[(T, u32)]) -> T {
    let total: u32 = options.iter().map(|(_, w)| *w).sum();
    if total == 0 {
        // Even-odds fallback (shouldn't happen with current trait tables).
        return options[rng.gen_range(0..options.len())].0;
    }
    let mut roll: u32 = rng.gen_range(0..total);
    for (value, weight) in options {
        if roll < *weight {
            return *value;
        }
        roll -= *weight;
    }
    options[options.len() - 1].0
}

/// Resolve trait choices: for each `None` ("any") slot, pick a weighted-random
/// value using the same option weights the canonical algorithm uses.
pub fn resolve_choices<R: Rng + ?Sized>(
    choices: &TraitChoices,
    rng: &mut R,
) -> Result<ResolvedChoices> {
    macro_rules! resolve {
        ($field:ident, $parser:expr, $opts:expr) => {{
            match &choices.$field {
                Some(s) => $parser(s)
                    .with_context(|| format!("parsing {}", stringify!($field)))?,
                None => pick_weighted(rng, $opts),
            }
        }};
    }
    let bull = |c: &Option<bool>, rng: &mut R| match c {
        Some(b) => *b,
        None => rng.gen_bool(0.5),
    };
    let mut bullseye1 = bull(&choices.bullseye1, rng);
    let mut bullseye3 = bull(&choices.bullseye3, rng);
    let mut bullseye7 = bull(&choices.bullseye7, rng);
    // Apply the "bullseye 2" derived constraint:
    //   Some(true)  → all three of 1/3/7 forced off so the algorithm falls
    //                 back to 2-ring bullseyes.
    //   Some(false) → guarantee at least one of 1/3/7 is on so the fallback
    //                 doesn't trigger; if everything resolved to off, flip a
    //                 random one to true.
    match choices.bullseye2 {
        Some(true) => {
            bullseye1 = false;
            bullseye3 = false;
            bullseye7 = false;
        }
        Some(false) => {
            if !bullseye1 && !bullseye3 && !bullseye7 {
                match rng.gen_range(0..3u8) {
                    0 => bullseye1 = true,
                    1 => bullseye3 = true,
                    _ => bullseye7 = true,
                }
            }
        }
        None => {}
    }
    Ok(ResolvedChoices {
        flow_field: resolve!(flow_field, parse_flow_field, q::FlowField::options()),
        turbulence: resolve!(turbulence, parse_turbulence, q::Turbulence::options()),
        margin: resolve!(margin, parse_margin, q::Margin::options()),
        color_variety: resolve!(
            color_variety,
            parse_color_variety,
            q::ColorVariety::options()
        ),
        color_mode: resolve!(color_mode, parse_color_mode, q::ColorMode::options()),
        structure: resolve!(structure, parse_structure, q::Structure::options()),
        bullseye1,
        bullseye3,
        bullseye7,
        ring_thickness: resolve!(
            ring_thickness,
            parse_ring_thickness,
            q::RingThickness::options()
        ),
        ring_size: resolve!(ring_size, parse_ring_size, q::RingSize::options()),
        size_variety: resolve!(
            size_variety,
            parse_size_variety,
            q::SizeVariety::options()
        ),
        color_palette: resolve!(
            color_palette,
            parse_color_palette,
            q::ColorPalette::options()
        ),
        spacing: resolve!(spacing, parse_spacing, q::Spacing::options()),
    })
}

// ---------------------------------------------------------------------------
// Address + seed construction

pub fn parse_address(s: &str) -> Result<[u8; 20]> {
    let s = s.trim().trim_start_matches("0x");
    if s.len() != 40 {
        bail!("ETH address must be 40 hex chars (got {})", s.len());
    }
    let bytes = hex::decode(s).context("invalid hex in address")?;
    Ok(bytes.try_into().unwrap())
}

pub fn parse_seed_hex(s: &str) -> Result<[u8; 32]> {
    let s = s.trim().trim_start_matches("0x");
    if s.len() != 64 {
        bail!("seed must be 64 hex chars (got {})", s.len());
    }
    let bytes = hex::decode(s).context("invalid hex in seed")?;
    Ok(bytes.try_into().unwrap())
}

pub fn seed_to_hex(seed: &[u8; 32]) -> String {
    format!("0x{}", hex::encode(seed))
}

/// Assemble a 32-byte seed from address, 6-byte nonce, and resolved trait choices.
pub fn build_seed(
    address: &[u8; 20],
    nonce: &[u8; 6],
    choices: &ResolvedChoices,
) -> Result<[u8; 32]> {
    let mut seed = [0u8; 32];
    seed[0..20].copy_from_slice(address);
    seed[20..26].copy_from_slice(nonce);
    seed[26] = 0xff;
    seed[27] = 0xff;
    let word = encode_trait_word(choices)?;
    seed[28..32].copy_from_slice(&word.to_be_bytes());
    Ok(seed)
}

pub fn random_nonce<R: RngCore>(rng: &mut R) -> [u8; 6] {
    let mut n = [0u8; 6];
    rng.fill_bytes(&mut n);
    n
}

/// Check whether the seed's decoded traits satisfy all non-None filter slots.
pub fn matches_filter(traits: &q::Traits, filter: &TraitChoices) -> bool {
    macro_rules! check_str {
        ($field:ident, $value:expr) => {
            match &filter.$field {
                Some(want) if want != &format!("{:?}", $value) => return false,
                _ => {}
            }
        };
    }
    check_str!(flow_field, traits.flow_field);
    check_str!(turbulence, traits.turbulence);
    check_str!(margin, traits.margin);
    check_str!(color_variety, traits.color_variety);
    check_str!(color_mode, traits.color_mode);
    check_str!(structure, traits.structure);
    if let Some(b) = filter.bullseye1 {
        if traits.bullseye_rings.one != b {
            return false;
        }
    }
    if let Some(b) = filter.bullseye3 {
        if traits.bullseye_rings.three != b {
            return false;
        }
    }
    if let Some(b) = filter.bullseye7 {
        if traits.bullseye_rings.seven != b {
            return false;
        }
    }
    // Derived "bullseye 2" constraint: matches when all three of 1/3/7 are off.
    if let Some(b) = filter.bullseye2 {
        let all_off = !traits.bullseye_rings.one
            && !traits.bullseye_rings.three
            && !traits.bullseye_rings.seven;
        if all_off != b {
            return false;
        }
    }
    check_str!(ring_thickness, traits.ring_thickness);
    check_str!(ring_size, traits.ring_size);
    check_str!(size_variety, traits.size_variety);
    check_str!(color_palette, traits.color_palette);
    check_str!(spacing, traits.spacing);
    true
}

// ---------------------------------------------------------------------------
// Trait option lists (for the UI to render dropdowns without hardcoding)

#[derive(Serialize)]
pub struct TraitMeta {
    pub name: &'static str,
    pub options: Vec<&'static str>,
}

pub fn trait_metadata() -> Vec<TraitMeta> {
    fn names<T>(opts: &[(T, u32)]) -> Vec<&'static str>
    where
        T: std::fmt::Debug,
    {
        opts.iter()
            .map(|(v, _)| -> &'static str {
                // Leak the debug string so it lives 'static. Cheap & one-time.
                Box::leak(format!("{:?}", v).into_boxed_str())
            })
            .collect()
    }
    vec![
        TraitMeta {
            name: "flowField",
            options: names(q::FlowField::options()),
        },
        TraitMeta {
            name: "turbulence",
            options: names(q::Turbulence::options()),
        },
        TraitMeta {
            name: "margin",
            options: names(q::Margin::options()),
        },
        TraitMeta {
            name: "colorVariety",
            options: names(q::ColorVariety::options()),
        },
        TraitMeta {
            name: "colorMode",
            options: names(q::ColorMode::options()),
        },
        TraitMeta {
            name: "structure",
            options: names(q::Structure::options()),
        },
        TraitMeta {
            name: "ringThickness",
            options: names(q::RingThickness::options()),
        },
        TraitMeta {
            name: "ringSize",
            options: names(q::RingSize::options()),
        },
        TraitMeta {
            name: "sizeVariety",
            options: names(q::SizeVariety::options()),
        },
        TraitMeta {
            name: "colorPalette",
            options: names(q::ColorPalette::options()),
        },
        TraitMeta {
            name: "spacing",
            options: names(q::Spacing::options()),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use hex_literal::hex;

    /// Round-trip: build a seed from the canonical QQL #219 trait selections and
    /// verify the trait-encoding portion matches.
    #[test]
    fn encode_matches_known_seed() {
        let known = hex!("33c9371d25ce44a408f8a6473fbad86bf81e1a17a2e52c90cf66ffff1296712e");
        let traits = q::Traits::from_seed(&known);
        let choices = ResolvedChoices {
            flow_field: traits.flow_field,
            turbulence: traits.turbulence,
            margin: traits.margin,
            color_variety: traits.color_variety,
            color_mode: traits.color_mode,
            structure: traits.structure,
            bullseye1: traits.bullseye_rings.one,
            bullseye3: traits.bullseye_rings.three,
            bullseye7: traits.bullseye_rings.seven,
            ring_thickness: traits.ring_thickness,
            ring_size: traits.ring_size,
            size_variety: traits.size_variety,
            color_palette: traits.color_palette,
            spacing: traits.spacing,
        };
        let mut address = [0u8; 20];
        address.copy_from_slice(&known[0..20]);
        let mut nonce = [0u8; 6];
        nonce.copy_from_slice(&known[20..26]);
        let built = build_seed(&address, &nonce, &choices).unwrap();
        // Bytes 0..28 must match (address + nonce + sentinel).
        assert_eq!(&built[0..28], &known[0..28]);
        // Last 4 bytes: our re-encoded word may differ in bits the canonical
        // encoder happened to use, but decoding it must yield the same traits.
        let reencoded_traits = q::Traits::from_seed(&built);
        assert_eq!(reencoded_traits, traits);
    }
}

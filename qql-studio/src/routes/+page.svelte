<script lang="ts">
	import { onDestroy, onMount } from 'svelte';
	import { listen, type UnlistenFn } from '@tauri-apps/api/event';
	import {
		cancelSearch,
		decodeSeed,
		deleteSeedPng,
		emptyChoices,
		generateCandidates,
		listPaletteColors,
		sessionId,
		listFavorites,
		listTraits,
		saveSeedPng,
		type Candidate,
		type CharacteristicFilter,
		type ColorInfo,
		type GenerateMode,
		type GenerateProgress,
		type PaletteColors,
		type TraitChoices,
		type TraitMeta
	} from '$lib/api';
	import { open as openDialog } from '@tauri-apps/plugin-dialog';
	import Tile from '$lib/Tile.svelte';
	import FilterInfo from '$lib/FilterInfo.svelte';
	import Detail from '$lib/Detail.svelte';
	import { dropQueuedRenders, setRenderConcurrency } from '$lib/renderQueue';

	const LS_KEY = 'qql-studio.v1';

	let address = $state('');
	let choices = $state<TraitChoices>(emptyChoices());

	// Favorites: hearting a tile renders the seed at the chosen size and writes
	// it to `saveFolder`; un-hearting deletes that file. QQL's native sizes are
	// 4:5, so we only pick the width and the backend derives the height.
	const SAVE_SIZES: [number, string][] = [
		[800, '800 × 1000'],
		[2400, '2400 × 3000'],
		[4800, '4800 × 6000']
	];
	let saveFolder = $state<string | null>(null);
	let saveWidth = $state<number>(2400);
	// Seeds currently saved to disk (drives the filled heart). Reassigned on
	// change so Svelte picks it up.
	let favorites = $state<Set<string>>(new Set());

	// Each structure-specific filter section only matches its own Structure, so
	// setting one while a different Structure is chosen guarantees zero results.
	// These flags grey out (and skip from the request) the sections that don't
	// apply. "Any" structure leaves all three enabled. The values map to the
	// `Structure` enum names surfaced by trait metadata.
	const formationEnabled = $derived(
		!choices.structure || choices.structure === 'Formation'
	);
	const orbitalEnabled = $derived(
		!choices.structure || choices.structure === 'Orbital'
	);
	const shadowsEnabled = $derived(
		!choices.structure || choices.structure === 'Shadows'
	);
	// The flow-angle filter only applies to linear flow fields. Radial flows
	// (Explosive/Spiral/Circular/RandomRadial) have no single line angle, so an
	// angle range would reject every seed. We grey out + reset it when a radial
	// flow is explicitly chosen; "Any" keeps it enabled (it implicitly keeps the
	// linear seeds in range).
	const RADIAL_FLOWS = ['Explosive', 'Spiral', 'Circular', 'RandomRadial'];
	const angleApplies = $derived(!RADIAL_FLOWS.includes(choices.flowField ?? ''));
	// Horizontal/Diagonal/Vertical are pinned to a single angle, so the range
	// slider can't narrow anything — surface that rather than silently letting
	// the user set a range that does nothing. RandomLinear is the free one.
	const FIXED_ANGLES: Record<string, number> = {
		Horizontal: 0,
		Diagonal: 45,
		Vertical: 90
	};
	const fixedAngle = $derived(FIXED_ANGLES[choices.flowField ?? '']);
	// Spiral center is the mirror image: only radial flows curl around a
	// center, so it greys out when a linear flow is explicitly chosen.
	const LINEAR_FLOWS = ['Horizontal', 'Diagonal', 'Vertical', 'RandomLinear'];
	const centerApplies = $derived(!LINEAR_FLOWS.includes(choices.flowField ?? ''));
	// Construct-only: traits are encoded directly into each seed. (The Search
	// mode was removed from the UI; the backend still supports it.)
	const mode: GenerateMode = 'construct';
	let count = $state(12);
	let maxAttempts = $state<number | null>(null);
	let workers = $state<number>(8);
	let thumbWidth = $state<number>(720);
	let runLayout = $state(false);
	let layoutTimeoutEnabled = $state<boolean>(false);
	let layoutTimeoutSeconds = $state<number>(10);

	// Display width follows render resolution: half of the thumb width gives
	// pixel-perfect crispness on a 2× Retina display.
	const tileDisplayWidth = $derived(Math.round(thumbWidth / 2));

	// When a palette is pinned, only that palette's backgrounds are reachable;
	// with Palette = Any, every palette's backgrounds are offered, grouped.
	const backgroundGroups = $derived.by(() =>
		paletteColors.filter((p) => !choices.colorPalette || p.palette === choices.colorPalette)
	);
	// Picks the pinned palette can't produce are kept (they come back if you
	// switch palette again) but not sent, so a hidden pick can't reject seeds.
	const activeBackgrounds = $derived.by(() => {
		const reachable = new Set(backgroundGroups.flatMap((p) => p.backgrounds.map((b) => b.name)));
		return backgroundColors.filter((b) => reachable.has(b));
	});
	function toggleBackground(name: string) {
		backgroundColors = backgroundColors.includes(name)
			? backgroundColors.filter((c) => c !== name)
			: [...backgroundColors, name];
	}
	// "Fidenza Newsprint" → "Newsprint" inside the Fidenza group.
	const shortBgName = (palette: string, name: string) =>
		name.startsWith(palette + ' ') ? name.slice(palette.length + 1) : name;
	const oddsPct = (odds: number) => `${Math.round(odds * 100)}%`;
	const visiblePrimaries = $derived.by(() => {
		const selected = choices.colorPalette;
		const map = new Map<string, ColorInfo>();
		for (const p of paletteColors) {
			if (selected && p.palette !== selected) continue;
			for (const c of p.primaries) {
				map.set(c.name, c);
			}
		}
		return [...map.values()].sort((a, b) => a.name.localeCompare(b.name));
	});

	// Same for ring colors.
	const activeRingColors = $derived.by(() => {
		const reachable = new Set(visiblePrimaries.map((c) => c.name));
		return ringColors.filter((c) => reachable.has(c));
	});

	function toggleRingColor(name: string) {
		if (ringColors.includes(name)) {
			ringColors = ringColors.filter((c) => c !== name);
		} else {
			ringColors = [...ringColors, name];
		}
	}
	let minPoints = $state<number | null>(null);
	let maxPoints = $state<number | null>(null);
	let minColors = $state<number | null>(null);
	let maxColors = $state<number | null>(null);
	let minSmall = $state<number | null>(null);
	let maxSmall = $state<number | null>(null);
	let minMedium = $state<number | null>(null);
	let maxMedium = $state<number | null>(null);
	let minLarge = $state<number | null>(null);
	let maxLarge = $state<number | null>(null);
	let minAngle = $state<number>(0);
	let maxAngle = $state<number>(90);
	// When the angle filter doesn't apply (radial flow chosen) it's kept but not
	// sent (see angleActive), so switching back restores the range you had.
	// Spiral center bounds, in sixths of the canvas (0 = left/top edge,
	// 6 = right/bottom edge). Centers range from −⅔ to 5⁄3 and the fixed
	// choices all land on sixths, so −4…10 is the full, unconstrained range.
	const CENTER_LO = -4;
	const CENTER_HI = 10;
	let centerXMin = $state<number>(CENTER_LO);
	let centerXMax = $state<number>(CENTER_HI);
	let centerYMin = $state<number>(CENTER_LO);
	let centerYMax = $state<number>(CENTER_HI);
	function setCenter(lo: number, hi: number) {
		centerXMin = centerYMin = lo;
		centerXMax = centerYMax = hi;
	}
	const centerActive = $derived(
		centerApplies &&
			(centerXMin > CENTER_LO ||
				centerXMax < CENTER_HI ||
				centerYMin > CENTER_LO ||
				centerYMax < CENTER_HI)
	);
	// Likewise the spiral center is kept but not sent while a linear flow is
	// pinned (centerActive already requires centerApplies).
	const sixthPct = (k: number) => `${Math.round((k / 6) * 100)}%`;
	const centerPos = (k: number) => ((k - CENTER_LO) / (CENTER_HI - CENTER_LO)) * 100;
	let cornerConcentration = $state<'' | 'TL' | 'TR' | 'BL' | 'BR' | 'AnyLeft' | 'AnyRight'>('');
	let cornerRatio = $state<number>(1.5);
	let minHorizontalSections = $state<number | null>(null);
	let maxHorizontalSections = $state<number | null>(null);
	let minVerticalSections = $state<number | null>(null);
	let maxVerticalSections = $state<number | null>(null);
	// Formation point spacing (`step`) checkboxes, basis points of canvas width.
	let fStep75 = $state<boolean>(false); // 0.75%
	let fStep100 = $state<boolean>(false); // 1%
	let fStep200 = $state<boolean>(false); // 2%
	let fStep400 = $state<boolean>(false); // 4%
	let fStep800 = $state<boolean>(false); // 8%
	// Formation skip-odds checkboxes (basis points: 0 / 1000 / 2000 / 5000).
	let fSkip0 = $state<boolean>(false); // none
	let fSkip10 = $state<boolean>(false); // 0.1
	let fSkip20 = $state<boolean>(false); // 0.2
	let fSkip50 = $state<boolean>(false); // 0.5
	let minFormationChunks = $state<number | null>(null);
	let maxFormationChunks = $state<number | null>(null);
	// Actual chunks can't exceed the grid of starting chunks. Sections come
	// from {1,2,3,4,5,7}; "no cap" means up to 7. Quirk of the algorithm's
	// float loop: 7 vertical sections give 8 rows (the 8th is off-canvas), so
	// the largest grid is 7 × 8 = 56. (Horizontal has no such quirk.)
	const largestSection = (cap: number | null) =>
		Math.max(1, ...[1, 2, 3, 4, 5, 7].filter((v) => v <= (cap ?? 7)));
	const maxAchievableChunks = $derived.by(() => {
		const v = largestSection(maxVerticalSections);
		return largestSection(maxHorizontalSections) * (v === 7 ? 8 : v);
	});
	const chunksImpossible = $derived(
		formationEnabled &&
			minFormationChunks != null &&
			minFormationChunks > maxAchievableChunks
	);
	// Provably-impossible filter combinations, surfaced inline and blocked at
	// Generate. Only deterministic impossibilities go here (not merely-rare ones).
	// (Called from onGenerate, so it may reference filter state declared below.)
	function impossibleReasons(cf: CharacteristicFilter): string[] {
		const reasons: string[] = [];
		if (count == null || !Number.isFinite(count) || count < 1) {
			reasons.push('Count: enter how many seeds to find (1 or more).');
		} else if (count > 2048) {
			reasons.push('Count: the most one search can return is 2,048.');
		}
		if (chunksImpossible) {
			reasons.push(
				`Actual chunks: minimum is ${minFormationChunks}, but with Horizontal ≤ ${maxHorizontalSections ?? 7} and Vertical ≤ ${maxVerticalSections ?? 7}, the most chunks a Formation can have is ${maxAchievableChunks}. Lower the minimum, or raise the section limits.`
			);
		}
		// Any min > max range matches nothing. Only check pairs whose section
		// applies to the chosen Structure — greyed-out filters are dropped from
		// the request anyway, so a stale inverted range there is harmless.
		const minMaxPairs: [number | null, number | null, string, boolean][] = [
			[minPoints, maxPoints, 'Points', true],
			[minColors, maxColors, 'Colors', true],
			[minSmall, maxSmall, 'Small radius bucket', true],
			[minMedium, maxMedium, 'Medium radius bucket', true],
			[minLarge, maxLarge, 'Large radius bucket', true],
			[minSplatterOdds, maxSplatterOdds, 'Splatter odds', true],
			[minHorizontalSections, maxHorizontalSections, 'Horizontal sections', formationEnabled],
			[minVerticalSections, maxVerticalSections, 'Vertical sections', formationEnabled],
			[minFormationChunks, maxFormationChunks, 'Actual chunks', formationEnabled],
			[minRingBands, maxRingBands, 'Ring bands', orbitalEnabled],
			[minSplitOffsetDeg, maxSplitOffsetDeg, 'Split offset', orbitalEnabled],
			[minShadowsActualCircles, maxShadowsActualCircles, 'Actual circles', shadowsEnabled]
		];
		for (const [min, max, label, applies] of minMaxPairs) {
			if (applies && ((min != null && min < 0) || (max != null && max < 0))) {
				reasons.push(`${label}: values can't be negative.`);
			} else if (applies && min != null && max != null && min > max) {
				reasons.push(`${label}: minimum (${min}) is greater than maximum (${max}).`);
			}
		}

		// Flow angle needs a linear flow; Spiral center needs a radial one.
		const angleOn = angleApplies && (minAngle > 0 || maxAngle < 90);
		if (angleOn && centerActive) {
			reasons.push(
				'Flow angle and Spiral center are both set, but the angle only exists on linear flows and the center only on radial ones, so no seed can match both. Clear one, or pin a Flow field.'
			);
		}
		// A Horizontal/Diagonal/Vertical flow has exactly one angle.
		if (angleOn && fixedAngle !== undefined && (fixedAngle < minAngle || fixedAngle > maxAngle)) {
			reasons.push(
				`Flow angle: ${choices.flowField} is always ${fixedAngle}°, which is outside ${minAngle}°–${maxAngle}°. Widen the range or use RandomLinear.`
			);
		}

		// With Structure = Any, settings on two structure cards can never both
		// apply: a seed has exactly one structure.
		const has = (...keys: (keyof CharacteristicFilter)[]) =>
			keys.some((k) => cf[k] !== undefined && cf[k] !== null);
		const cardsSet = [
			has(
				'minHorizontalSections',
				'maxHorizontalSections',
				'minVerticalSections',
				'maxVerticalSections',
				'allowedFormationStepBp',
				'allowedFormationSkipBp',
				'minFormationChunks',
				'maxFormationChunks'
			) && 'Formation',
			has(
				'minRingBands',
				'maxRingBands',
				'allowedOrbitalSplits',
				'requiredOrbitalSplits',
				'allowedOrbitalCenterXPositions',
				'allowedOrbitalCenterYPositions',
				'allowedOrbitalBaseStepBp',
				'allowedOrbitalRadialStepBp',
				'minSplitOffsetDeg',
				'maxSplitOffsetDeg'
			) && 'Orbital',
			has(
				'allowedShadowsNumCircles',
				'shadowsFillStyle',
				'minShadowsActualCircles',
				'maxShadowsActualCircles',
				'shadowsColumnarSquare',
				'shadowsOutwardRadial'
			) && 'Shadows'
		].filter(Boolean);
		if (cardsSet.length > 1) {
			reasons.push(
				`Settings on the ${cardsSet.join(' and ')} cards can't all apply: a seed has only one structure. Clear all but one card, or pin a Structure.`
			);
		}

		// Every required split must also be allowed.
		const allowed = cf.allowedOrbitalSplits;
		const missing = (cf.requiredOrbitalSplits ?? []).filter((r) => allowed && !allowed.includes(r));
		if (allowed && missing.length > 0) {
			reasons.push(
				`Orbital splits: ${missing.join(', ')} is required but not allowed. Tick it under Allowed splits too.`
			);
		}

		// Splatter odds come from Color variety: Low tops out at 0.005 and
		// Medium at 0.03 (see qqlrs-main art.rs splatter table).
		const variety = choices.colorVariety;
		const maxOdds = variety === 'Low' ? 0.005 : variety === 'Medium' ? 0.03 : undefined;
		const presetMin = splatterMode === 'moderate' ? 0.01 : splatterMode === 'heavy' ? 0.08 : undefined;
		if (maxOdds !== undefined && presetMin !== undefined && presetMin > maxOdds) {
			reasons.push(
				`Splatter: ${splatterMode === 'heavy' ? 'Heavy+' : 'Moderate+'} can't happen with Color variety ${variety}. Pick a lighter preset or a higher variety.`
			);
		}
		return reasons;
	}
	let minRingBands = $state<number | null>(null);
	let maxRingBands = $state<number | null>(null);
	let allowOrbitalSplit1 = $state<boolean>(false);
	let allowOrbitalSplit2 = $state<boolean>(false);
	let allowOrbitalSplit3 = $state<boolean>(false);
	let requireOrbitalSplit1 = $state<boolean>(false);
	let requireOrbitalSplit2 = $state<boolean>(false);
	let requireOrbitalSplit3 = $state<boolean>(false);
	// Orbital center: allowed fixed positions per axis, as codes -3..3 in
	// canvas order (see ORBITAL_CENTER_POSITIONS below).
	let orbitalCenterX = $state<number[]>([]);
	let orbitalCenterY = $state<number[]>([]);
	const ORBITAL_CENTER_POSITIONS = [
		{ code: -3, label: 'far', x: 'way off the left edge (−160%)', y: 'way above the top (−160%)' },
		{ code: -2, label: 'out', x: 'just off the left edge (−33%)', y: 'just above the top (−33%)' },
		{ code: -1, label: '⅓', x: 'left third (33%)', y: 'upper third (33% down)' },
		{ code: 0, label: '½', x: 'centre (50%)', y: 'centre (50%)' },
		{ code: 1, label: '⅔', x: 'right third (67%)', y: 'lower third (67% down)' },
		{ code: 2, label: 'out', x: 'just off the right edge (133%)', y: 'just below the bottom (133%)' },
		{ code: 3, label: 'far', x: 'way off the right edge (160%)', y: 'way below the bottom (160%)' }
	];
	const togglePos = (list: number[], code: number) =>
		list.includes(code) ? list.filter((c) => c !== code) : [...list, code];
	// Point spacing checkboxes (basis points of canvas width).
	let bs1 = $state<boolean>(false); // 1%
	let bs2 = $state<boolean>(false); // 2%
	let bs4 = $state<boolean>(false); // 4%
	let bs6 = $state<boolean>(false); // 6%
	let bs8 = $state<boolean>(false); // 8%
	let bs16 = $state<boolean>(false); // 16%
	// Ring band thickness checkboxes.
	let rs7 = $state<boolean>(false); // 7%
	let rs15 = $state<boolean>(false); // 15%
	let rs30 = $state<boolean>(false); // 30%
	// Curve length checkboxes (applies to all structures).
	let cl500 = $state<boolean>(false);
	let cl650 = $state<boolean>(false);
	let cl850 = $state<boolean>(false);
	// Orbital split offset bounds in degrees (rarely useful).
	let minSplitOffsetDeg = $state<number | null>(null);
	let maxSplitOffsetDeg = $state<number | null>(null);
	let backgroundColors = $state<string[]>([]);
	let ringColors = $state<string[]>([]);
	let exactRingColors = $state<boolean>(false);
	let splatterMode = $state<'' | 'none' | 'any' | 'light' | 'moderate' | 'heavy'>('');
	let minSplatterOdds = $state<number | null>(null);
	let maxSplatterOdds = $state<number | null>(null);
	// Shadows structure
	let sc5 = $state<boolean>(false);
	let sc7 = $state<boolean>(false);
	let sc10 = $state<boolean>(false);
	let sc20 = $state<boolean>(false);
	let sc30 = $state<boolean>(false);
	let sc60 = $state<boolean>(false);
	let shadowsFillStyle = $state<'' | 'all_radial' | 'mixed' | 'all_square'>('');
	let minShadowsActualCircles = $state<number | null>(null);
	let maxShadowsActualCircles = $state<number | null>(null);
	let shadowsColumnar = $state<'' | 'col' | 'row'>('');
	let shadowsRadialDir = $state<'' | 'out' | 'in'>('');
	let paletteColors = $state<PaletteColors[]>([]);

	let traitsMeta = $state<TraitMeta[]>([]);
	let results = $state<Candidate[]>([]);
	let busy = $state(false);
	let cancelling = $state(false);
	let progress = $state<GenerateProgress | null>(null);
	let statusMsg = $state('');
	// When the current search started (for hit rate / time estimates), and a
	// summary of the last finished search (for the "no matches" screen).
	let searchStartedAt = $state<number | null>(null);
	let lastSearch = $state<{
		attempts: number;
		maxAttempts: number;
		cancelled: boolean;
		timedOut: number;
	} | null>(null);
	// Results view: optional sort and favorites-only toggle.
	let sortBy = $state<'found' | 'pointsDesc' | 'pointsAsc' | 'colorsDesc'>('found');
	let favoritesOnly = $state(false);
	let error = $state<string | null>(null);
	let openDetail = $state<Candidate | null>(null);
	let inspectHex = $state('');
	let inspectErr = $state<string | null>(null);
	let inspecting = $state(false);
	let unlisten: UnlistenFn | null = null;
	let renderedCount = $state(0);
	let renderRunStart = $state<number | null>(null);
	let renderRunDuration = $state<number | null>(null);

	type StringKey = Exclude<
		keyof TraitChoices,
		'bullseye1' | 'bullseye3' | 'bullseye7'
	>;
	// Order matches qql.art/create. Palette → Color Mode → Color Variety →
	// Structure → Direction (Flow field) → Turbulence → Margin → Ring Size →
	// Size Variety → Spacing → Rings (rendered separately) → Ring Thickness.
	const stringTraitFieldsTop: [StringKey, string][] = [
		['colorPalette', 'Color palette'],
		['colorMode', 'Color mode'],
		['colorVariety', 'Color variety'],
		['structure', 'Structure'],
		['flowField', 'Flow field'],
		['turbulence', 'Turbulence'],
		['margin', 'Margin'],
		['ringSize', 'Ring size'],
		['sizeVariety', 'Size variety'],
		['spacing', 'Spacing']
	];
	const stringTraitFieldsBottom: [StringKey, string][] = [
		['ringThickness', 'Ring thickness']
	];
	type BullKey = 'bullseye1' | 'bullseye2' | 'bullseye3' | 'bullseye7';
	const bullseyeFields: [BullKey, string][] = [
		['bullseye1', '1'],
		['bullseye2', '2'],
		['bullseye3', '3'],
		['bullseye7', '7']
	];

	// Allowed values the algorithm can actually produce, so the inputs can't
	// offer numbers that never occur.
	// Formation grid: num_horizontal/vertical_steps are weighted choices from
	// {1,2,3,4,5,7} (note: no 6). See qqlrs-main/src/layouts.rs `formation()`.
	const FORMATION_SECTION_VALUES = [1, 2, 3, 4, 5, 7];

	// ---- Remembered filters ------------------------------------------------
	// Every filter setting, with its default. Saving, restoring and "Reset all
	// filters" all walk this one list, so a filter can't be left out of any.
	type FilterField = [get: () => unknown, set: (v: any) => void, def: () => unknown];
	const FILTER_FIELDS: Record<string, FilterField> = {
		minPoints: [() => minPoints, (v) => (minPoints = v), () => null],
		maxPoints: [() => maxPoints, (v) => (maxPoints = v), () => null],
		minColors: [() => minColors, (v) => (minColors = v), () => null],
		maxColors: [() => maxColors, (v) => (maxColors = v), () => null],
		minSmall: [() => minSmall, (v) => (minSmall = v), () => null],
		maxSmall: [() => maxSmall, (v) => (maxSmall = v), () => null],
		minMedium: [() => minMedium, (v) => (minMedium = v), () => null],
		maxMedium: [() => maxMedium, (v) => (maxMedium = v), () => null],
		minLarge: [() => minLarge, (v) => (minLarge = v), () => null],
		maxLarge: [() => maxLarge, (v) => (maxLarge = v), () => null],
		minAngle: [() => minAngle, (v) => (minAngle = v), () => 0],
		maxAngle: [() => maxAngle, (v) => (maxAngle = v), () => 90],
		centerXMin: [() => centerXMin, (v) => (centerXMin = v), () => CENTER_LO],
		centerXMax: [() => centerXMax, (v) => (centerXMax = v), () => CENTER_HI],
		centerYMin: [() => centerYMin, (v) => (centerYMin = v), () => CENTER_LO],
		centerYMax: [() => centerYMax, (v) => (centerYMax = v), () => CENTER_HI],
		cornerConcentration: [() => cornerConcentration, (v) => (cornerConcentration = v), () => ''],
		cornerRatio: [() => cornerRatio, (v) => (cornerRatio = v), () => 1.5],
		minHorizontalSections: [() => minHorizontalSections, (v) => (minHorizontalSections = v), () => null],
		maxHorizontalSections: [() => maxHorizontalSections, (v) => (maxHorizontalSections = v), () => null],
		minVerticalSections: [() => minVerticalSections, (v) => (minVerticalSections = v), () => null],
		maxVerticalSections: [() => maxVerticalSections, (v) => (maxVerticalSections = v), () => null],
		fStep75: [() => fStep75, (v) => (fStep75 = v), () => false],
		fStep100: [() => fStep100, (v) => (fStep100 = v), () => false],
		fStep200: [() => fStep200, (v) => (fStep200 = v), () => false],
		fStep400: [() => fStep400, (v) => (fStep400 = v), () => false],
		fStep800: [() => fStep800, (v) => (fStep800 = v), () => false],
		fSkip0: [() => fSkip0, (v) => (fSkip0 = v), () => false],
		fSkip10: [() => fSkip10, (v) => (fSkip10 = v), () => false],
		fSkip20: [() => fSkip20, (v) => (fSkip20 = v), () => false],
		fSkip50: [() => fSkip50, (v) => (fSkip50 = v), () => false],
		minFormationChunks: [() => minFormationChunks, (v) => (minFormationChunks = v), () => null],
		maxFormationChunks: [() => maxFormationChunks, (v) => (maxFormationChunks = v), () => null],
		minRingBands: [() => minRingBands, (v) => (minRingBands = v), () => null],
		maxRingBands: [() => maxRingBands, (v) => (maxRingBands = v), () => null],
		allowOrbitalSplit1: [() => allowOrbitalSplit1, (v) => (allowOrbitalSplit1 = v), () => false],
		allowOrbitalSplit2: [() => allowOrbitalSplit2, (v) => (allowOrbitalSplit2 = v), () => false],
		allowOrbitalSplit3: [() => allowOrbitalSplit3, (v) => (allowOrbitalSplit3 = v), () => false],
		requireOrbitalSplit1: [() => requireOrbitalSplit1, (v) => (requireOrbitalSplit1 = v), () => false],
		requireOrbitalSplit2: [() => requireOrbitalSplit2, (v) => (requireOrbitalSplit2 = v), () => false],
		requireOrbitalSplit3: [() => requireOrbitalSplit3, (v) => (requireOrbitalSplit3 = v), () => false],
		orbitalCenterX: [() => orbitalCenterX, (v) => (orbitalCenterX = v), () => []],
		orbitalCenterY: [() => orbitalCenterY, (v) => (orbitalCenterY = v), () => []],
		bs1: [() => bs1, (v) => (bs1 = v), () => false],
		bs2: [() => bs2, (v) => (bs2 = v), () => false],
		bs4: [() => bs4, (v) => (bs4 = v), () => false],
		bs6: [() => bs6, (v) => (bs6 = v), () => false],
		bs8: [() => bs8, (v) => (bs8 = v), () => false],
		bs16: [() => bs16, (v) => (bs16 = v), () => false],
		rs7: [() => rs7, (v) => (rs7 = v), () => false],
		rs15: [() => rs15, (v) => (rs15 = v), () => false],
		rs30: [() => rs30, (v) => (rs30 = v), () => false],
		cl500: [() => cl500, (v) => (cl500 = v), () => false],
		cl650: [() => cl650, (v) => (cl650 = v), () => false],
		cl850: [() => cl850, (v) => (cl850 = v), () => false],
		minSplitOffsetDeg: [() => minSplitOffsetDeg, (v) => (minSplitOffsetDeg = v), () => null],
		maxSplitOffsetDeg: [() => maxSplitOffsetDeg, (v) => (maxSplitOffsetDeg = v), () => null],
		backgroundColors: [() => backgroundColors, (v) => (backgroundColors = v), () => []],
		ringColors: [() => ringColors, (v) => (ringColors = v), () => []],
		exactRingColors: [() => exactRingColors, (v) => (exactRingColors = v), () => false],
		splatterMode: [() => splatterMode, (v) => (splatterMode = v), () => ''],
		minSplatterOdds: [() => minSplatterOdds, (v) => (minSplatterOdds = v), () => null],
		maxSplatterOdds: [() => maxSplatterOdds, (v) => (maxSplatterOdds = v), () => null],
		sc5: [() => sc5, (v) => (sc5 = v), () => false],
		sc7: [() => sc7, (v) => (sc7 = v), () => false],
		sc10: [() => sc10, (v) => (sc10 = v), () => false],
		sc20: [() => sc20, (v) => (sc20 = v), () => false],
		sc30: [() => sc30, (v) => (sc30 = v), () => false],
		sc60: [() => sc60, (v) => (sc60 = v), () => false],
		shadowsFillStyle: [() => shadowsFillStyle, (v) => (shadowsFillStyle = v), () => ''],
		minShadowsActualCircles: [() => minShadowsActualCircles, (v) => (minShadowsActualCircles = v), () => null],
		maxShadowsActualCircles: [() => maxShadowsActualCircles, (v) => (maxShadowsActualCircles = v), () => null],
		shadowsColumnar: [() => shadowsColumnar, (v) => (shadowsColumnar = v), () => ''],
		shadowsRadialDir: [() => shadowsRadialDir, (v) => (shadowsRadialDir = v), () => ''],
	};
	const FILTERS_KEY = 'qql-studio.filters.v1';
	// A saved value is used only if it has the same shape as the default.
	function sameShape(def: unknown, v: unknown): boolean {
		if (Array.isArray(def)) return Array.isArray(v);
		if (def === null) return v === null || typeof v === 'number';
		return typeof v === typeof def;
	}
	function restoreFilters(saved: Record<string, unknown>) {
		for (const [key, [, set, def]] of Object.entries(FILTER_FIELDS)) {
			if (key in saved && sameShape(def(), saved[key])) set(saved[key]);
		}
	}
	function resetAllFilters() {
		for (const [, set, def] of Object.values(FILTER_FIELDS)) set(def());
	}
	$effect(() => {
		const snap: Record<string, unknown> = {};
		for (const [key, [get]] of Object.entries(FILTER_FIELDS)) snap[key] = get();
		if (!filtersRestored) return;
		try {
			localStorage.setItem(FILTERS_KEY, JSON.stringify(snap));
		} catch {}
	});
	// Don't save until the saved filters have been read back in on launch.
	let filtersRestored = $state(false);

	onMount(async () => {
		try {
			const persisted = localStorage.getItem(LS_KEY);
			if (persisted) {
				const s = JSON.parse(persisted);
				address = s.address ?? '';
				choices = { ...emptyChoices(), ...(s.choices ?? {}) };
				count = s.count ?? 12;
				maxAttempts = s.maxAttempts ?? null;
				workers = typeof s.workers === 'number' ? s.workers : 8;
				thumbWidth = typeof s.thumbWidth === 'number' ? s.thumbWidth : 720;
				minAngle = typeof s.minAngle === 'number' ? s.minAngle : 0;
				maxAngle = typeof s.maxAngle === 'number' ? s.maxAngle : 90;
				const num = (v: unknown, d: number) => (typeof v === 'number' ? v : d);
				centerXMin = num(s.centerXMin, CENTER_LO);
				centerXMax = num(s.centerXMax, CENTER_HI);
				centerYMin = num(s.centerYMin, CENTER_LO);
				centerYMax = num(s.centerYMax, CENTER_HI);
				layoutTimeoutEnabled =
					typeof s.layoutTimeoutEnabled === 'boolean'
						? s.layoutTimeoutEnabled
						: false;
				layoutTimeoutSeconds =
					typeof s.layoutTimeoutSeconds === 'number'
						? s.layoutTimeoutSeconds
						: 10;
				runLayout = s.runLayout ?? false;
				saveFolder = typeof s.saveFolder === 'string' ? s.saveFolder : null;
				saveWidth = typeof s.saveWidth === 'number' ? s.saveWidth : 2400;
			}
			// Filters saved by this version (older versions kept only the angle
			// and spiral center, read above from the settings).
			const savedFilters = localStorage.getItem(FILTERS_KEY);
			if (savedFilters) restoreFilters(JSON.parse(savedFilters));
		} catch {}
		filtersRestored = true;
		traitsMeta = await listTraits();
		paletteColors = await listPaletteColors();
		unlisten = await listen<GenerateProgress>('search-progress', (event) => {
			progress = event.payload;
			// Show seeds as they're found. The final result list replaces this
			// when the search ends (same seeds, same order), so late or missed
			// updates can't leave anything out.
			const fresh = event.payload.newCandidates ?? [];
			if (busy && fresh.length > 0) {
				const have = new Set(results.map((r) => r.seed));
				const add = fresh.filter((c) => !have.has(c.seed));
				if (add.length > 0) {
					results = [...results, ...add];
					if (renderRunStart === null) renderRunStart = performance.now();
				}
			}
		});
		await restoreAfterReload();
	});

	// Hearts mirror the PNGs actually in the save folder, so they survive a
	// restart and follow the folder if it changes.
	$effect(() => {
		const folder = saveFolder;
		if (!folder) {
			favorites = new Set();
			return;
		}
		listFavorites(folder)
			.then((seeds) => {
				if (saveFolder === folder) favorites = new Set(seeds);
			})
			.catch(() => {});
	});

	// ---- Survive a webview reload ------------------------------------------
	// WebKit kills and silently reloads the page when it uses too much memory
	// while hidden (e.g. lots of tiles, laptop asleep). The app itself keeps
	// running, so we save the results tagged with the app's session id and put
	// them back if the page comes up again within the same session. A fresh
	// launch has a new id and starts empty as before.
	const RESULTS_KEY = 'qql-studio.results.v1';
	let appSession = $state<string | null>(null);

	async function restoreAfterReload() {
		try {
			const id = await sessionId();
			// Read before setting appSession, which lets the save effect run.
			const raw = localStorage.getItem(RESULTS_KEY);
			appSession = id;
			if (!raw || results.length > 0) return;
			const snap = JSON.parse(raw);
			if (snap.session !== appSession || !Array.isArray(snap.results)) return;
			if (snap.results.length === 0) return;
			results = snap.results;
			statusMsg = `Restored ${results.length} results after the view reloaded (it can run out of memory while hidden).`;
			renderRunStart = performance.now();
		} catch {}
	}

	$effect(() => {
		if (!appSession || busy) return;
		const snap = { session: appSession, results };
		try {
			localStorage.setItem(RESULTS_KEY, JSON.stringify(snap));
		} catch {
			// Over the storage quota: keep seeds and traits, drop the stats.
			try {
				const lean = { ...snap, results: results.map((r) => ({ ...r, stats: null })) };
				localStorage.setItem(RESULTS_KEY, JSON.stringify(lean));
			} catch {
				// Never leave an older snapshot behind to be restored by mistake.
				localStorage.removeItem(RESULTS_KEY);
			}
		}
	});

	onDestroy(() => {
		unlisten?.();
	});

	$effect(() => {
		try {
			localStorage.setItem(
				LS_KEY,
				JSON.stringify({
					address,
					choices,
					count,
					maxAttempts,
					workers,
					thumbWidth,
					layoutTimeoutEnabled,
					layoutTimeoutSeconds,
					runLayout,
					saveFolder,
					saveWidth
				})
			);
		} catch {}
	});

	$effect(() => {
		setRenderConcurrency(workers);
	});

	function metaFor(name: string): string[] {
		return traitsMeta.find((m) => m.name === name)?.options ?? [];
	}

	// Whole, non-negative number for a count field, or undefined when empty.
	// Number inputs can hold decimals (and negatives are caught by
	// impossibleReasons); the backend expects unsigned integers.
	function whole(v: number | null | undefined): number | undefined {
		return v == null || !Number.isFinite(v) ? undefined : Math.max(0, Math.floor(v));
	}

	// The characteristic filter as sent to the backend: empty controls become
	// undefined, and cards that don't apply to the chosen Structure are dropped.
	function buildCharFilter(): CharacteristicFilter {
		const charFilter: CharacteristicFilter = {
			minPoints: whole(minPoints),
			maxPoints: whole(maxPoints),
			minColors: whole(minColors),
			maxColors: whole(maxColors),
			minSmall: whole(minSmall),
			maxSmall: whole(maxSmall),
			minMedium: whole(minMedium),
			maxMedium: whole(maxMedium),
			minLarge: whole(minLarge),
			maxLarge: whole(maxLarge),
			cornerConcentration: cornerConcentration || undefined,
			cornerRatio: cornerConcentration ? cornerRatio : undefined,
			minHorizontalSections: whole(minHorizontalSections),
			maxHorizontalSections: whole(maxHorizontalSections),
			minVerticalSections: whole(minVerticalSections),
			maxVerticalSections: whole(maxVerticalSections),
			allowedFormationStepBp: (() => {
				const list = [
					fStep75 ? 75 : null,
					fStep100 ? 100 : null,
					fStep200 ? 200 : null,
					fStep400 ? 400 : null,
					fStep800 ? 800 : null
				].filter((v): v is number => v !== null);
				return list.length > 0 ? list : undefined;
			})(),
			allowedFormationSkipBp: (() => {
				const list = [
					fSkip0 ? 0 : null,
					fSkip10 ? 1000 : null,
					fSkip20 ? 2000 : null,
					fSkip50 ? 5000 : null
				].filter((v): v is number => v !== null);
				return list.length > 0 ? list : undefined;
			})(),
			minFormationChunks: whole(minFormationChunks),
			maxFormationChunks: whole(maxFormationChunks),
			minRingBands: whole(minRingBands),
			maxRingBands: whole(maxRingBands),
			allowedOrbitalSplits: (() => {
				const list = [
					allowOrbitalSplit1 ? 1 : null,
					allowOrbitalSplit2 ? 2 : null,
					allowOrbitalSplit3 ? 3 : null
				].filter((v): v is number => v !== null);
				return list.length > 0 ? list : undefined;
			})(),
			requiredOrbitalSplits: (() => {
				const list = [
					requireOrbitalSplit1 ? 1 : null,
					requireOrbitalSplit2 ? 2 : null,
					requireOrbitalSplit3 ? 3 : null
				].filter((v): v is number => v !== null);
				return list.length > 0 ? list : undefined;
			})(),
			allowedOrbitalCenterXPositions: orbitalCenterX.length > 0 ? orbitalCenterX : undefined,
			allowedOrbitalCenterYPositions: orbitalCenterY.length > 0 ? orbitalCenterY : undefined,
			allowedOrbitalBaseStepBp: (() => {
				const list = [
					bs1 ? 100 : null,
					bs2 ? 200 : null,
					bs4 ? 400 : null,
					bs6 ? 600 : null,
					bs8 ? 800 : null,
					bs16 ? 1600 : null
				].filter((v): v is number => v !== null);
				return list.length > 0 ? list : undefined;
			})(),
			allowedOrbitalRadialStepBp: (() => {
				const list = [
					rs7 ? 700 : null,
					rs15 ? 1500 : null,
					rs30 ? 3000 : null
				].filter((v): v is number => v !== null);
				return list.length > 0 ? list : undefined;
			})(),
			allowedCurveLengths: (() => {
				const list = [
					cl500 ? 500 : null,
					cl650 ? 650 : null,
					cl850 ? 850 : null
				].filter((v): v is number => v !== null);
				return list.length > 0 ? list : undefined;
			})(),
			minSplitOffsetDeg: minSplitOffsetDeg ?? undefined,
			maxSplitOffsetDeg: maxSplitOffsetDeg ?? undefined,
			backgroundColors: activeBackgrounds.length > 0 ? activeBackgrounds : undefined,
			ringColors: activeRingColors.length > 0 ? activeRingColors : undefined,
			exactRingColors: activeRingColors.length > 0 ? exactRingColors : undefined,
			splatterMode: splatterMode || undefined,
			minSplatterOdds: minSplatterOdds ?? undefined,
			maxSplatterOdds: maxSplatterOdds ?? undefined,
			allowedShadowsNumCircles: (() => {
				const list = [
					sc5 ? 5 : null,
					sc7 ? 7 : null,
					sc10 ? 10 : null,
					sc20 ? 20 : null,
					sc30 ? 30 : null,
					sc60 ? 60 : null
				].filter((v): v is number => v !== null);
				return list.length > 0 ? list : undefined;
			})(),
			shadowsFillStyle: shadowsFillStyle || undefined,
			minShadowsActualCircles: whole(minShadowsActualCircles),
			maxShadowsActualCircles: whole(maxShadowsActualCircles),
			shadowsColumnarSquare:
				shadowsColumnar === 'col'
					? true
					: shadowsColumnar === 'row'
						? false
						: undefined,
			shadowsOutwardRadial:
				shadowsRadialDir === 'out'
					? true
					: shadowsRadialDir === 'in'
						? false
						: undefined
		};
		// Drop filters for structure sections that don't apply to the chosen
		// Structure, so a greyed-out (or stale) value can't force zero results.
		if (!formationEnabled) {
			charFilter.minHorizontalSections = undefined;
			charFilter.maxHorizontalSections = undefined;
			charFilter.minVerticalSections = undefined;
			charFilter.maxVerticalSections = undefined;
			charFilter.allowedFormationStepBp = undefined;
			charFilter.allowedFormationSkipBp = undefined;
			charFilter.minFormationChunks = undefined;
			charFilter.maxFormationChunks = undefined;
		}
		if (!orbitalEnabled) {
			charFilter.minRingBands = undefined;
			charFilter.maxRingBands = undefined;
			charFilter.allowedOrbitalSplits = undefined;
			charFilter.requiredOrbitalSplits = undefined;
			charFilter.allowedOrbitalCenterXPositions = undefined;
			charFilter.allowedOrbitalCenterYPositions = undefined;
			charFilter.allowedOrbitalBaseStepBp = undefined;
			charFilter.allowedOrbitalRadialStepBp = undefined;
			charFilter.minSplitOffsetDeg = undefined;
			charFilter.maxSplitOffsetDeg = undefined;
		}
		if (!shadowsEnabled) {
			charFilter.allowedShadowsNumCircles = undefined;
			charFilter.shadowsFillStyle = undefined;
			charFilter.minShadowsActualCircles = undefined;
			charFilter.maxShadowsActualCircles = undefined;
			charFilter.shadowsColumnarSquare = undefined;
			charFilter.shadowsOutwardRadial = undefined;
		}
		return charFilter;
	}

	async function onGenerate() {
		error = null;
		statusMsg = '';
		const cf = buildCharFilter();
		// Bail before churning attempts on a provably-impossible combination.
		const reasons = impossibleReasons(cf);
		if (reasons.length > 0) {
			error = `This combination can't produce results:\n• ${reasons.join('\n• ')}`;
			return;
		}
		progress = null;
		results = [];
		lastSearch = null;
		searchStartedAt = performance.now();
		dropQueuedRenders();
		renderedCount = 0;
		renderRunStart = null;
		renderRunDuration = null;
		cancelling = false;
		busy = true;
		try {
			const charFilter = cf;
			const isCharFilterActive =
				Object.values(charFilter).some((v) => v !== undefined && v !== null);
			const angleActive = angleApplies && (minAngle > 0 || maxAngle < 90);
			const outcome = await generateCandidates({
				address,
				choices,
				mode,
				count: Math.floor(count),
				maxAttempts: whole(maxAttempts),
				workers: workers >= 1 ? Math.floor(workers) : undefined,
				// The backend lays out a seed only when a point-level filter needs
				// it; structure filters are checked without one.
				runLayout,
				characteristicFilter: isCharFilterActive ? charFilter : undefined,
				minAngleDeg: angleActive ? minAngle : undefined,
				maxAngleDeg: angleActive ? maxAngle : undefined,
				minCenterX: centerActive ? centerXMin / 6 : undefined,
				maxCenterX: centerActive ? centerXMax / 6 : undefined,
				minCenterY: centerActive ? centerYMin / 6 : undefined,
				maxCenterY: centerActive ? centerYMax / 6 : undefined,
				layoutTimeoutMs:
					layoutTimeoutEnabled && layoutTimeoutSeconds > 0
						? Math.round(layoutTimeoutSeconds * 1000)
						: undefined
			});
			results = outcome.candidates;
			lastSearch = {
				attempts: outcome.attempts,
				maxAttempts: outcome.maxAttempts,
				cancelled: outcome.cancelled,
				timedOut: outcome.timedOut
			};
			const rate = results.length > 0 ? ` (1 in ${hitRate(outcome.attempts, results.length)})` : '';
			const timeouts =
				outcome.timedOut > 0
					? ` ${outcome.timedOut.toLocaleString()} hit the layout timeout.`
					: '';
			if (outcome.cancelled) {
				statusMsg = `Cancelled. Got ${results.length}/${count} after ${outcome.attempts.toLocaleString()} attempts${rate}.${timeouts}`;
			} else if (results.length < count) {
				statusMsg = `Got ${results.length}/${count} after ${outcome.attempts.toLocaleString()} attempts${rate} (cap ${outcome.maxAttempts.toLocaleString()}). Try widening filters or raise max attempts.${timeouts}`;
			} else {
				statusMsg = `Found ${results.length} in ${outcome.attempts.toLocaleString()} attempts${rate}.${timeouts}`;
			}
			if (results.length > 0 && renderRunStart === null) {
				renderRunStart = performance.now();
			}
		} catch (e) {
			error = String(e);
		} finally {
			busy = false;
			cancelling = false;
		}
	}

	// "1 in N", rounded to something readable.
	function hitRate(attempts: number, found: number): string {
		const n = attempts / Math.max(1, found);
		return n >= 100 ? Math.round(n).toLocaleString() : n >= 10 ? n.toFixed(0) : n.toFixed(1);
	}

	// Rough time left, from the pace so far. Null until something is found.
	const searchEta = $derived.by(() => {
		if (!busy || !progress || progress.found === 0 || searchStartedAt === null) return null;
		const elapsed = (performance.now() - searchStartedAt) / 1000;
		const left = (elapsed / progress.found) * (progress.want - progress.found);
		if (left < 1) return null;
		return left < 90 ? `~${Math.round(left)}s left` : `~${Math.round(left / 60)} min left`;
	});

	// The grid order: as found, or sorted by stats (seeds without stats go to
	// the end). "♥ only" hides tiles with CSS rather than removing them, so
	// their rendered images survive toggling it off again.
	const displayed = $derived.by(() => {
		let list = results;
		if (sortBy !== 'found') {
			const key = (r: Candidate) =>
				r.stats == null
					? null
					: sortBy === 'colorsDesc'
						? r.stats.colorsUsed.length
						: r.stats.numPoints;
			const dir = sortBy === 'pointsAsc' ? 1 : -1;
			list = [...list].sort((a, b) => {
				const ka = key(a), kb = key(b);
				if (ka == null || kb == null) return (ka == null ? 1 : 0) - (kb == null ? 1 : 0);
				return (ka - kb) * dir;
			});
		}
		return list;
	});
	const anyStats = $derived(results.some((r) => r.stats != null));

	// Previous / next in the inspector walk the grid as currently shown.
	const navList = $derived(
		favoritesOnly ? displayed.filter((r) => favorites.has(r.seed)) : displayed
	);
	const navIndex = $derived(
		openDetail ? navList.findIndex((r) => r.seed === openDetail!.seed) : -1
	);
	function stepDetail(delta: number) {
		const i = navIndex + delta;
		if (navIndex >= 0 && i >= 0 && i < navList.length) openDetail = navList[i];
	}

	async function onCancel() {
		if (cancelling) return;
		cancelling = true;
		try {
			await cancelSearch();
		} catch {}
	}

	// Seeds in the current results; renders finishing for an older search's
	// tiles must not count toward this one.
	const resultSeeds = $derived(new Set(results.map((r) => r.seed)));
	function onTileRendered(seed: string) {
		if (!resultSeeds.has(seed)) return;
		renderedCount += 1;
		if (
			renderedCount >= results.length &&
			renderRunStart !== null &&
			renderRunDuration === null
		) {
			renderRunDuration = performance.now() - renderRunStart;
		}
	}

	function resetTraits() {
		choices = emptyChoices();
	}

	// Inspect any seed: decode its traits and open the Detail view (where it
	// can be rendered, animated, and exported). Accepts a bare seed or any
	// text containing one, such as a qql.art token link. Detail fills in the
	// layout stats from its own render, so we don't wait for a layout here.
	async function inspectSeed() {
		const raw = inspectHex.trim();
		if (!raw || inspecting) return;
		inspectErr = null;
		const hex = raw.match(/(?:0x)?([0-9a-fA-F]{64})(?![0-9a-fA-F])/)?.[1];
		if (!hex) {
			inspectErr = "Couldn't find a seed in that text. Paste a 64-character seed or a qql.art token link.";
			return;
		}
		inspecting = true;
		try {
			const seed = `0x${hex.toLowerCase()}`;
			const traits = await decodeSeed(seed);
			openDetail = { seed, traits, stats: null };
		} catch (e) {
			inspectErr = String(e);
		} finally {
			inspecting = false;
		}
	}

	async function chooseSaveFolder() {
		const picked = await openDialog({
			directory: true,
			multiple: false,
			title: 'Choose a folder for saved favorites'
		});
		if (typeof picked === 'string') {
			saveFolder = picked;
		}
	}

	// Returns the folder to save into, prompting for one if not set yet. Returns
	// null if the user cancels the picker.
	async function ensureSaveFolder(): Promise<string | null> {
		if (saveFolder) return saveFolder;
		await chooseSaveFolder();
		return saveFolder;
	}

	async function toggleFavorite(seed: string) {
		if (favorites.has(seed)) {
			// Un-heart: delete the file we wrote (decision: folder mirrors hearts).
			if (saveFolder) {
				try {
					await deleteSeedPng(saveFolder, seed);
				} catch (e) {
					error = `Couldn't delete favorite: ${e}`;
					return;
				}
			}
			favorites.delete(seed);
			favorites = new Set(favorites);
		} else {
			const folder = await ensureSaveFolder();
			if (!folder) return; // user cancelled the picker
			try {
				await saveSeedPng(seed, saveWidth, folder);
			} catch (e) {
				error = `Couldn't save favorite: ${e}`;
				return;
			}
			favorites.add(seed);
			favorites = new Set(favorites);
		}
	}

	// "2 rings" is the algorithm's fallback that only occurs when 1/3/7 are all
	// off, so it's mutually exclusive with them. Keep the Rings control from
	// reaching a combination that can never match a seed: choosing 2 = yes
	// forces the others to no, and choosing any of 1/3/7 = yes forces 2 to no.
	function onBullseyeChange(key: BullKey) {
		if (key === 'bullseye2') {
			if (choices.bullseye2 === true) {
				choices.bullseye1 = false;
				choices.bullseye3 = false;
				choices.bullseye7 = false;
			}
		} else if (choices[key] === true && choices.bullseye2 === true) {
			choices.bullseye2 = false;
		}
	}
</script>

<svelte:head>
	<title>QQL Studio</title>
</svelte:head>

<div class="app">
	<aside class="controls">
		<header>
			<h1>QQL Studio</h1>
			<p class="sub">Trait-filtered seed explorer</p>
		</header>

		<section class="group group-setup">
			<label class="block">
				<span class="label">ETH address</span>
				<input
					type="text"
					bind:value={address}
					placeholder="0x…"
					spellcheck="false"
					autocomplete="off"
				/>
			</label>
			<div class="block">
				<span class="label">Save folder <small>for ♥ favorites</small></span>
				<div class="folder-row">
					<button class="ghost" onclick={chooseSaveFolder}>Choose…</button>
					<span class="folder-path" class:none={!saveFolder} title={saveFolder ?? ''}>
						{saveFolder ?? 'none selected'}
					</span>
				</div>
			</div>
			<label class="block">
				<span class="label">Save size <small>PNG written on ♥</small></span>
				<select bind:value={saveWidth}>
					{#each SAVE_SIZES as [w, label]}
						<option value={w}>{label}</option>
					{/each}
				</select>
			</label>
			<div class="block">
				<span class="label">Inspect a seed <small>render / animate any seed</small></span>
				<div class="folder-row">
					<input
						type="text"
						bind:value={inspectHex}
						placeholder="0x… seed hex"
						spellcheck="false"
						autocomplete="off"
						onkeydown={(e) => e.key === 'Enter' && inspectSeed()}
					/>
					<button class="ghost" onclick={inspectSeed} disabled={inspecting || !inspectHex.trim()}>
						{inspecting ? '…' : 'View'}
					</button>
				</div>
				{#if inspectErr}<div class="inspect-err">{inspectErr}</div>{/if}
			</div>
		</section>

		<section class="group group-traits">
			<div class="section-head">
				<h2>Traits</h2>
				<button class="ghost" onclick={resetTraits}>reset</button>
			</div>
			<div class="grid">
				{#each stringTraitFieldsTop as [key, label]}
					<label class="block">
						<span class="label">{label}</span>
						<select bind:value={choices[key]}>
							<option value={null}>Any</option>
							{#each metaFor(key) as opt}
								<option value={opt}>{opt}</option>
							{/each}
						</select>
					</label>
				{/each}
			</div>

			<div class="bullseye">
				<div class="label">Rings</div>
				<div class="bullseye-row">
					{#each bullseyeFields as [key, label]}
						<label class="bull">
							<span>{label}</span>
							<select bind:value={choices[key]} onchange={() => onBullseyeChange(key)}>
								<option value={null}>Any</option>
								<option value={true}>yes</option>
								<option value={false}>no</option>
							</select>
						</label>
					{/each}
				</div>
			</div>

			<div class="grid">
				{#each stringTraitFieldsBottom as [key, label]}
					<label class="block">
						<span class="label">{label}</span>
						<select bind:value={choices[key]}>
							<option value={null}>Any</option>
							{#each metaFor(key) as opt}
								<option value={opt}>{opt}</option>
							{/each}
						</select>
					</label>
				{/each}
			</div>
		</section>

		<div class="filters-bar">
			<span>Filters <small>remembered between sessions</small></span>
			<button class="ghost" onclick={resetAllFilters}>Reset all filters</button>
		</div>

		<section class="group group-layout">
			<div class="section-head">
				<h2>Characteristics</h2>
				<small class="hint">requires layout</small>
			</div>
			<div class="char-grid">
				<label class="block">
					<span class="label">Min points</span>
					<input type="number" bind:value={minPoints} placeholder="—" min="0" />
				</label>
				<label class="block">
					<span class="label">Max points</span>
					<input type="number" bind:value={maxPoints} placeholder="—" min="0" />
				</label>
				<label class="block">
					<span class="label">Min colors</span>
					<input type="number" bind:value={minColors} placeholder="—" min="1" max="16" />
				</label>
				<label class="block">
					<span class="label">Max colors</span>
					<input type="number" bind:value={maxColors} placeholder="—" min="1" max="16" />
				</label>
			</div>
			<label class="checkbox">
				<input type="checkbox" bind:checked={runLayout} />
				<span>Always run layout (show points/colors per tile)</span>
			</label>
		</section>

		<section class="group group-layout">
			<div class="section-head">
				<h2>Radius buckets</h2>
				<small class="hint">requires layout</small>
			</div>
			<div class="bucket-row">
				<div class="bucket-label">
					<strong>Small</strong>
					<small>≤ 1.2% canvas</small>
				</div>
				<input
					type="number"
					bind:value={minSmall}
					placeholder="min"
					min="0"
				/>
				<input
					type="number"
					bind:value={maxSmall}
					placeholder="max"
					min="0"
				/>
			</div>
			<div class="bucket-row">
				<div class="bucket-label">
					<strong>Medium</strong>
					<small>1.2 – 3% canvas</small>
				</div>
				<input
					type="number"
					bind:value={minMedium}
					placeholder="min"
					min="0"
				/>
				<input
					type="number"
					bind:value={maxMedium}
					placeholder="max"
					min="0"
				/>
			</div>
			<div class="bucket-row">
				<div class="bucket-label">
					<strong>Large</strong>
					<small>&gt; 3% canvas</small>
				</div>
				<input
					type="number"
					bind:value={minLarge}
					placeholder="min"
					min="0"
				/>
				<input
					type="number"
					bind:value={maxLarge}
					placeholder="max"
					min="0"
				/>
			</div>
		</section>

		<section class="group group-colors">
			<div class="section-head">
				<h2>Colors</h2>
				<small class="hint">ring colors need layout</small>
			</div>

			<div class="subhead">Background <small>fast</small></div>
			{#each backgroundGroups as group (group.palette)}
				{#if !choices.colorPalette}
					<div class="bg-group-label">{group.palette}</div>
				{/if}
				<div class="color-chips-grid">
					{#each group.backgrounds as c (c.name)}
						<button
							type="button"
							class="chip-button"
							class:selected={backgroundColors.includes(c.name)}
							onclick={() => toggleBackground(c.name)}
							title="{c.name} — picked for {oddsPct(c.odds)} of {group.palette} seeds"
						>
							<span class="swatch" style="background:{c.swatch}"></span>
							<span class="chip-name">{shortBgName(group.palette, c.name)}</span>
							<span class="chip-odds">{oddsPct(c.odds)}</span>
						</button>
					{/each}
				</div>
			{/each}
			{#if activeBackgrounds.length > 0}
				<button class="ghost" onclick={() => (backgroundColors = [])}>
					clear backgrounds ({activeBackgrounds.length})
				</button>
			{:else}
				<small class="bg-hint">none selected = any background · % = how often it's picked in its palette</small>
			{/if}

			<div class="subhead">Ring colors</div>
			<div class="color-chips-grid">
				{#each visiblePrimaries as c}
					<button
						type="button"
						class="chip-button"
						class:selected={ringColors.includes(c.name)}
						onclick={() => toggleRingColor(c.name)}
					>
						<span class="swatch" style="background:{c.swatch}"></span>
						<span class="chip-name">{c.name}</span>
					</button>
				{/each}
			</div>
			{#if activeRingColors.length > 0}
				<label class="checkbox">
					<input type="checkbox" bind:checked={exactRingColors} />
					<span>Only these colors (exclude seeds using others)</span>
				</label>
				<button
					class="ghost"
					onclick={() => {
						ringColors = [];
						exactRingColors = false;
					}}
				>
					clear ring colors
				</button>
			{/if}

			<div class="subhead">Splatter <small>fast</small></div>
			<label class="block">
				<select bind:value={splatterMode}>
					<option value="">Any</option>
					<option value="none">None (no splatter at all)</option>
					<option value="any">Has splatter (odds &gt; 0)</option>
					<option value="light">Light only (≤ 0.005)</option>
					<option value="moderate">Moderate+ (≥ 0.01)</option>
					<option value="heavy">Heavy+ (≥ 0.08)</option>
				</select>
			</label>
			<div class="bucket-row">
				<div class="bucket-label">
					<strong>Odds range</strong>
					<small>0–0.5 · advanced</small>
				</div>
				<input
					type="number"
					bind:value={minSplatterOdds}
					placeholder="min"
					step="0.001"
					min="0"
					max="0.5"
				/>
				<input
					type="number"
					bind:value={maxSplatterOdds}
					placeholder="max"
					step="0.001"
					min="0"
					max="0.5"
				/>
			</div>
		</section>

		<section class="group group-composition">
			<div class="section-head">
				<h2>Composition</h2>
				<small class="hint">corner concentration needs layout</small>
			</div>

			<fieldset class="struct-group" disabled={!angleApplies}>
			<div class="subhead">
				Flow angle
				{@render flowAngleInfo()}
				{#if angleApplies}
					<small>best with RandomLinear</small>
				{:else}
					<small class="locked">n/a — radial flow has no line angle</small>
				{/if}
			</div>
			<div class="angle-head">
				<span class="angle-readout">{minAngle}° – {maxAngle}°</span>
				<button
					class="ghost"
					onclick={() => {
						minAngle = 0;
						maxAngle = 90;
					}}
				>
					reset
				</button>
			</div>
			<div class="dual-range">
				<div class="track"></div>
				<div
					class="track-fill"
					style="left: {(minAngle / 90) * 100}%; right: {100 -
						(maxAngle / 90) * 100}%"
				></div>
				<input
					class:raised={minAngle === maxAngle && minAngle > 45}
					type="range"
					min="0"
					max="90"
					step="1"
					bind:value={minAngle}
					oninput={() => {
						if (minAngle > maxAngle) minAngle = maxAngle;
					}}
					aria-label="Minimum angle"
				/>
				<input
					type="range"
					min="0"
					max="90"
					step="1"
					bind:value={maxAngle}
					oninput={() => {
						if (maxAngle < minAngle) maxAngle = minAngle;
					}}
					aria-label="Maximum angle"
				/>
			</div>
			<div class="angle-legend">
				<span>0° horizontal</span>
				<span>45° diagonal</span>
				<span>90° vertical</span>
			</div>
			{#if fixedAngle !== undefined}
				{#if fixedAngle >= minAngle && fixedAngle <= maxAngle}
					<div class="angle-note">
						{choices.flowField} is always {fixedAngle}°, so this range doesn't narrow
						anything. Use RandomLinear for angles in between.
					</div>
				{:else}
					<div class="angle-note warn">
						{choices.flowField} is always {fixedAngle}°, outside this range, so nothing
						will match. Widen the range or use RandomLinear.
					</div>
				{/if}
			{/if}
			</fieldset>

			<fieldset class="struct-group" disabled={!centerApplies}>
			<div class="subhead">
				Spiral center
				{@render spiralCenterInfo()}
				{#if !centerApplies}
					<small class="locked">n/a — linear flow has no center</small>
				{:else if !choices.flowField}
					<small>excludes linear flows when set</small>
				{/if}
			</div>
			<div class="center-picker">
				<svg class="center-map" viewBox="0 0 56 70" aria-hidden="true">
					<rect class="map-bg" x="0" y="0" width="56" height="70" />
					<rect
						class="map-sel"
						x={(centerXMin - CENTER_LO) * 4}
						y={(centerYMin - CENTER_LO) * 5}
						width={Math.max((centerXMax - centerXMin) * 4, 1)}
						height={Math.max((centerYMax - centerYMin) * 5, 1)}
					/>
					<rect class="map-canvas" x="16" y="20" width="24" height="30" />
				</svg>
				<div class="center-sliders">
					<div class="angle-head">
						<span class="center-axis">Left → right</span>
						<span class="angle-readout">{sixthPct(centerXMin)} – {sixthPct(centerXMax)}</span>
					</div>
					<div class="dual-range">
						<div class="track"></div>
						<div
							class="track-canvas"
							style="left: {centerPos(0)}%; right: {100 - centerPos(6)}%"
						></div>
						<div
							class="track-fill"
							style="left: {centerPos(centerXMin)}%; right: {100 - centerPos(centerXMax)}%"
						></div>
						<input
							class:raised={centerXMin === centerXMax && centerXMin > 3}
							type="range"
							min={CENTER_LO}
							max={CENTER_HI}
							step="1"
							bind:value={centerXMin}
							oninput={() => {
								if (centerXMin > centerXMax) centerXMin = centerXMax;
							}}
							aria-label="Spiral center leftmost position"
						/>
						<input
							type="range"
							min={CENTER_LO}
							max={CENTER_HI}
							step="1"
							bind:value={centerXMax}
							oninput={() => {
								if (centerXMax < centerXMin) centerXMax = centerXMin;
							}}
							aria-label="Spiral center rightmost position"
						/>
					</div>
					<div class="angle-head">
						<span class="center-axis">Top → bottom</span>
						<span class="angle-readout">{sixthPct(centerYMin)} – {sixthPct(centerYMax)}</span>
					</div>
					<div class="dual-range">
						<div class="track"></div>
						<div
							class="track-canvas"
							style="left: {centerPos(0)}%; right: {100 - centerPos(6)}%"
						></div>
						<div
							class="track-fill"
							style="left: {centerPos(centerYMin)}%; right: {100 - centerPos(centerYMax)}%"
						></div>
						<input
							class:raised={centerYMin === centerYMax && centerYMin > 3}
							type="range"
							min={CENTER_LO}
							max={CENTER_HI}
							step="1"
							bind:value={centerYMin}
							oninput={() => {
								if (centerYMin > centerYMax) centerYMin = centerYMax;
							}}
							aria-label="Spiral center topmost position"
						/>
						<input
							type="range"
							min={CENTER_LO}
							max={CENTER_HI}
							step="1"
							bind:value={centerYMax}
							oninput={() => {
								if (centerYMax < centerYMin) centerYMax = centerYMin;
							}}
							aria-label="Spiral center bottommost position"
						/>
					</div>
				</div>
			</div>
			<div class="center-presets">
				<button class="ghost" onclick={() => setCenter(CENTER_LO, CENTER_HI)}>anywhere</button>
				<button class="ghost" onclick={() => setCenter(0, 6)}>on canvas</button>
				<button class="ghost" onclick={() => setCenter(2, 4)}>middle</button>
			</div>
			</fieldset>

			<div class="subhead">Corner concentration</div>
			<label class="block">
				<select bind:value={cornerConcentration}>
					<option value="">Any (no filter)</option>
					<option value="TL">Top-left densest</option>
					<option value="TR">Top-right densest</option>
					<option value="BL">Bottom-left densest</option>
					<option value="BR">Bottom-right densest</option>
					<option value="AnyLeft">Densest on left (TL or BL)</option>
					<option value="AnyRight">Densest on right (TR or BR)</option>
				</select>
			</label>
			{#if cornerConcentration}
				<label class="block workers-row">
					<div class="workers-head">
						<span class="label">Min ratio (densest / sparsest)</span>
						<span class="workers-value">{cornerRatio.toFixed(2)}×</span>
					</div>
					<input
						type="range"
						min="1"
						max="3"
						step="0.05"
						bind:value={cornerRatio}
					/>
					<div class="workers-hint">
						1.0 = no gradient required; ~1.5× ≈ moderate; ~2.0× ≈ pronounced
					</div>
				</label>
			{/if}

			<div class="splits-row">
				<div class="splits-label">
					<strong>Curve length</strong>
					<small>applies to all structures · minor effect</small>
				</div>
				<div class="splits-checks">
					<label class="splits-check"><input type="checkbox" bind:checked={cl500} /><span>500</span></label>
					<label class="splits-check"><input type="checkbox" bind:checked={cl650} /><span>650</span></label>
					<label class="splits-check"><input type="checkbox" bind:checked={cl850} /><span>850</span></label>
				</div>
			</div>
		</section>

		<fieldset class="struct-group struct-card struct-formation" disabled={!formationEnabled}>
				<div class="section-head">
					<h2>Formation structure{@render formationInfo()}</h2>
					{#if formationEnabled}
						<small class="hint fast">fast · no layout</small>
					{:else}
						<small class="hint locked">n/a — Structure is {choices.structure}</small>
					{/if}
				</div>
			<div class="bucket-row">
				<div class="bucket-label">
					<strong>Horizontal{@render formationSectionsInfo()}</strong>
					<small>columns</small>
				</div>
				<select bind:value={minHorizontalSections}>
					<option value={null}>min</option>
					{#each FORMATION_SECTION_VALUES as v}
						<option value={v}>{v}</option>
					{/each}
				</select>
				<select bind:value={maxHorizontalSections}>
					<option value={null}>max</option>
					{#each FORMATION_SECTION_VALUES as v}
						<option value={v}>{v}</option>
					{/each}
				</select>
			</div>
			<div class="bucket-row">
				<div class="bucket-label">
					<strong>Vertical{@render formationSectionsInfo()}</strong>
					<small>rows</small>
				</div>
				<select bind:value={minVerticalSections}>
					<option value={null}>min</option>
					{#each FORMATION_SECTION_VALUES as v}
						<option value={v}>{v}</option>
					{/each}
				</select>
				<select bind:value={maxVerticalSections}>
					<option value={null}>max</option>
					{#each FORMATION_SECTION_VALUES as v}
						<option value={v}>{v}</option>
					{/each}
				</select>
			</div>
			<div class="splits-row">
				<div class="splits-label">
					<strong>Point spacing{@render formationSpacingInfo()}</strong>
					<small>gap between points (% canvas)</small>
				</div>
				<div class="splits-checks">
					<label class="splits-check"><input type="checkbox" bind:checked={fStep75} /><span>0.75</span></label>
					<label class="splits-check"><input type="checkbox" bind:checked={fStep100} /><span>1</span></label>
					<label class="splits-check"><input type="checkbox" bind:checked={fStep200} /><span>2</span></label>
					<label class="splits-check"><input type="checkbox" bind:checked={fStep400} /><span>4</span></label>
					<label class="splits-check"><input type="checkbox" bind:checked={fStep800} /><span>8</span></label>
				</div>
			</div>
			<div class="splits-row">
				<div class="splits-label">
					<strong>Skip odds{@render formationSkipInfo()}</strong>
					<small>chance each chunk is dropped</small>
				</div>
				<div class="splits-checks">
					<label class="splits-check"><input type="checkbox" bind:checked={fSkip0} /><span>0</span></label>
					<label class="splits-check"><input type="checkbox" bind:checked={fSkip10} /><span>.1</span></label>
					<label class="splits-check"><input type="checkbox" bind:checked={fSkip20} /><span>.2</span></label>
					<label class="splits-check"><input type="checkbox" bind:checked={fSkip50} /><span>.5</span></label>
				</div>
			</div>
			<div class="bucket-row">
				<div class="bucket-label">
					<strong>Actual chunks{@render formationChunksInfo()}</strong>
					<small>placed after skips</small>
				</div>
				<input type="number" bind:value={minFormationChunks} placeholder="min" min="1" max="56" />
				<input type="number" bind:value={maxFormationChunks} placeholder="max" min="1" max="56" />
			</div>
			{#if chunksImpossible}
				<div class="combo-warn">
					⚠ With Horizontal ≤ {maxHorizontalSections ?? 7} and Vertical ≤ {maxVerticalSections ??
						7}, the most chunks possible is {maxAchievableChunks} — a minimum above that
					matches nothing.
				</div>
			{/if}

		</fieldset>

		<fieldset class="struct-group struct-card struct-orbital" disabled={!orbitalEnabled}>
				<div class="section-head">
					<h2>Orbital structure{@render orbitalInfo()}</h2>
					{#if orbitalEnabled}
						<small class="hint fast">fast · no layout</small>
					{:else}
						<small class="hint locked">n/a — Structure is {choices.structure}</small>
					{/if}
				</div>
			<div class="bucket-row">
				<div class="bucket-label">
					<strong>Ring bands{@render ringBandsInfo()}</strong>
					<small>radial bands</small>
				</div>
				<input
					type="number"
					bind:value={minRingBands}
					placeholder="min"
					min="3"
					max="61"
				/>
				<input
					type="number"
					bind:value={maxRingBands}
					placeholder="max"
					min="3"
					max="61"
				/>
			</div>
			<div class="splits-row">
				<div class="splits-label">
					<strong>Allowed splits{@render allowedSplitsInfo()}</strong>
					<small>no band uses values outside checked</small>
				</div>
				<div class="splits-checks">
					<label class="splits-check">
						<input type="checkbox" bind:checked={allowOrbitalSplit1} />
						<span>1</span>
					</label>
					<label class="splits-check">
						<input type="checkbox" bind:checked={allowOrbitalSplit2} />
						<span>2</span>
					</label>
					<label class="splits-check">
						<input type="checkbox" bind:checked={allowOrbitalSplit3} />
						<span>3</span>
					</label>
				</div>
			</div>
			<div class="splits-row">
				<div class="splits-label">
					<strong>Required splits{@render requiredSplitsInfo()}</strong>
					<small>each checked must appear in some band</small>
				</div>
				<div class="splits-checks">
					<label class="splits-check">
						<input type="checkbox" bind:checked={requireOrbitalSplit1} />
						<span>1</span>
					</label>
					<label class="splits-check">
						<input type="checkbox" bind:checked={requireOrbitalSplit2} />
						<span>2</span>
					</label>
					<label class="splits-check">
						<input type="checkbox" bind:checked={requireOrbitalSplit3} />
						<span>3</span>
					</label>
				</div>
			</div>
			{#each [{ axis: 'x', name: 'Center X', before: '← off left', after: 'off right →' }, { axis: 'y', name: 'Center Y', before: '↑ above', after: 'below ↓' }] as ax (ax.axis)}
				{@const picked = ax.axis === 'x' ? orbitalCenterX : orbitalCenterY}
				<div class="center-pos">
					<div class="splits-label">
						<strong>{ax.name}{@render orbitalCenterInfo()}</strong>
						<small>where the rings radiate from</small>
					</div>
					<div class="center-pos-grid">
						{#each ORBITAL_CENTER_POSITIONS as p (p.code)}
							<label class="center-pos-cell" class:onCanvas={Math.abs(p.code) <= 1} title={ax.axis === 'x' ? p.x : p.y}>
								<input
									type="checkbox"
									checked={picked.includes(p.code)}
									onchange={() => {
										if (ax.axis === 'x') orbitalCenterX = togglePos(orbitalCenterX, p.code);
										else orbitalCenterY = togglePos(orbitalCenterY, p.code);
									}}
								/>
								<span>{p.label}</span>
							</label>
						{/each}
					</div>
					<div class="center-pos-legend">
						<span>{ax.before}</span><span>canvas</span><span>{ax.after}</span>
					</div>
				</div>
			{/each}
			<div class="splits-row">
				<div class="splits-label">
					<strong>Point spacing{@render orbitalSpacingInfo()}</strong>
					<small>distance along each ring (% canvas)</small>
				</div>
				<div class="splits-checks">
					<label class="splits-check"><input type="checkbox" bind:checked={bs1} /><span>1</span></label>
					<label class="splits-check"><input type="checkbox" bind:checked={bs2} /><span>2</span></label>
					<label class="splits-check"><input type="checkbox" bind:checked={bs4} /><span>4</span></label>
					<label class="splits-check"><input type="checkbox" bind:checked={bs6} /><span>6</span></label>
					<label class="splits-check"><input type="checkbox" bind:checked={bs8} /><span>8</span></label>
					<label class="splits-check"><input type="checkbox" bind:checked={bs16} /><span>16</span></label>
				</div>
			</div>
			<div class="splits-row">
				<div class="splits-label">
					<strong>Band thickness{@render bandThicknessInfo()}</strong>
					<small>ring band thickness (% canvas)</small>
				</div>
				<div class="splits-checks">
					<label class="splits-check"><input type="checkbox" bind:checked={rs7} /><span>7</span></label>
					<label class="splits-check"><input type="checkbox" bind:checked={rs15} /><span>15</span></label>
					<label class="splits-check"><input type="checkbox" bind:checked={rs30} /><span>30</span></label>
				</div>
			</div>
			<div class="bucket-row">
				<div class="bucket-label">
					<strong>Split offset{@render splitOffsetInfo()}</strong>
					<small>rotation only · rarely useful</small>
				</div>
				<input
					type="number"
					bind:value={minSplitOffsetDeg}
					placeholder="min°"
					min="0"
					max="360"
				/>
				<input
					type="number"
					bind:value={maxSplitOffsetDeg}
					placeholder="max°"
					min="0"
					max="360"
				/>
			</div>

		</fieldset>

		<fieldset class="struct-group struct-card struct-shadows" disabled={!shadowsEnabled}>
				<div class="section-head">
					<h2>Shadows structure{@render shadowsInfo()}</h2>
					{#if shadowsEnabled}
						<small class="hint fast">fast · no layout</small>
					{:else}
						<small class="hint locked">n/a — Structure is {choices.structure}</small>
					{/if}
				</div>
			<div class="splits-row">
				<div class="splits-label">
					<strong>Target circles{@render shadowsTargetInfo()}</strong>
					<small>algorithm's target count</small>
				</div>
				<div class="splits-checks">
					<label class="splits-check"><input type="checkbox" bind:checked={sc5} /><span>5</span></label>
					<label class="splits-check"><input type="checkbox" bind:checked={sc7} /><span>7</span></label>
					<label class="splits-check"><input type="checkbox" bind:checked={sc10} /><span>10</span></label>
					<label class="splits-check"><input type="checkbox" bind:checked={sc20} /><span>20</span></label>
					<label class="splits-check"><input type="checkbox" bind:checked={sc30} /><span>30</span></label>
					<label class="splits-check"><input type="checkbox" bind:checked={sc60} /><span>60</span></label>
				</div>
			</div>
			<label class="block">
				<span class="label">Fill style{@render shadowsFillInfo()}</span>
				<select bind:value={shadowsFillStyle}>
					<option value="">Any</option>
					<option value="all_radial">All radial (concentric rings)</option>
					<option value="mixed">Mixed (50/50 per circle)</option>
					<option value="all_square">All square (grids)</option>
				</select>
			</label>
			<div class="bucket-row">
				<div class="bucket-label">
					<strong>Actual circles{@render shadowsActualInfo()}</strong>
					<small>after collision rejection</small>
				</div>
				<input
					type="number"
					bind:value={minShadowsActualCircles}
					placeholder="min"
					min="0"
					max="60"
				/>
				<input
					type="number"
					bind:value={maxShadowsActualCircles}
					placeholder="max"
					min="0"
					max="60"
				/>
			</div>
			<label class="block">
				<span class="label">Square direction{@render shadowsSquareDirInfo()} <small>subtle · rarely useful</small></span>
				<select bind:value={shadowsColumnar}>
					<option value="">Any</option>
					<option value="col">Column-major</option>
					<option value="row">Row-major</option>
				</select>
			</label>
			<label class="block">
				<span class="label">Radial direction{@render shadowsRadialDirInfo()} <small>subtle · rarely useful</small></span>
				<select bind:value={shadowsRadialDir}>
					<option value="">Any</option>
					<option value="out">Outside → in</option>
					<option value="in">Inside → out</option>
				</select>
			</label>
		</fieldset>

		<section class="group group-generation">
			<div class="section-head">
				<h2>Generate</h2>
			</div>
			<div class="char-grid">
				<label class="block">
					<span class="label">Count</span>
					<input type="number" bind:value={count} min="1" max="2048" />
					{#if count > 400}
						<small class="count-warn">
							Large result sets use a lot of memory; if the view reloads, your
							results are restored.
						</small>
					{/if}
				</label>
				<label class="block">
					<span class="label">Max attempts</span>
					<input
						type="number"
						bind:value={maxAttempts}
						placeholder="auto"
						min="1"
					/>
				</label>
			</div>
			<label class="block workers-row">
				<div class="workers-head">
					<span class="label">Workers</span>
					<span class="workers-value">{workers}</span>
				</div>
				<input
					type="range"
					bind:value={workers}
					min="1"
					max="16"
					step="1"
				/>
				<div class="workers-hint">
					{#if workers <= 2}
						light — leaves most of your CPU free for other apps
					{:else if workers <= 6}
						balanced — comfortable for everyday use
					{:else if workers <= 10}
						fast — uses most of your CPU
					{:else}
						all-out — best when nothing else demanding is running
					{/if}
				</div>
			</label>
			<label class="block">
				<span class="label">Tile size</span>
				<select bind:value={thumbWidth}>
					<option value={480}>Small (240 px) — fastest</option>
					<option value={720}>Medium (360 px)</option>
					<option value={960}>Large (480 px)</option>
					<option value={1200}>Extra large (600 px)</option>
				</select>
			</label>

			<label class="checkbox">
				<input type="checkbox" bind:checked={layoutTimeoutEnabled} />
				<span>Layout timeout — skip seeds slower than this</span>
			</label>
			{#if layoutTimeoutEnabled}
				<label class="block timeout-row">
					<span class="label">Timeout (seconds)</span>
					<input
						type="number"
						bind:value={layoutTimeoutSeconds}
						min="0.5"
						step="0.5"
					/>
					<div class="timeout-hint">
						any single layout that exceeds this is abandoned and counted as
						a non-match. Only applies when a layout runs: point-level filters
						(points, colors, radius buckets, corner concentration, ring colors) or
						"Always run layout".
					</div>
				</label>
			{/if}
			{#if busy}
				<button class="cancel" onclick={onCancel} disabled={cancelling}>
					{cancelling ? 'Cancelling…' : 'Cancel'}
				</button>
			{:else}
				<button class="primary" onclick={onGenerate} disabled={!address}>
					Generate
				</button>
			{/if}
			{#if busy}
				<div class="progress">
					<div class="progress-line">
						<span class="phase">{cancelling ? 'Cancelling' : 'Generating'}</span>
						<span>
							Tried
							<strong>{(progress?.attempts ?? 0).toLocaleString()}</strong>
						</span>
						<span>·</span>
						<span>
							Found
							<strong>{progress?.found ?? 0}</strong> / {progress?.want ?? count}
						</span>
					</div>
					{#if progress && (progress.found > 0 || progress.timedOut > 0)}
						<div class="progress-line sub">
							{#if progress.found > 0}
								<span>1 in {hitRate(progress.attempts, progress.found)}</span>
							{/if}
							{#if searchEta}
								<span>· {searchEta}</span>
							{/if}
							{#if progress.timedOut > 0}
								<span>· {progress.timedOut.toLocaleString()} timed out</span>
							{/if}
						</div>
					{/if}
					<div class="bar">
						<div
							class="bar-fill"
							style="width: {progress
								? Math.min(
										100,
										(progress.attempts / Math.max(1, progress.maxAttempts)) *
											100
									)
								: 0}%"
						></div>
					</div>
				</div>
			{:else if results.length > 0 && renderedCount < results.length}
				<div class="progress">
					<div class="progress-line">
						<span class="phase">Rendering</span>
						<span>
							<strong>{renderedCount.toLocaleString()}</strong>
							/ {results.length}
						</span>
					</div>
					<div class="bar">
						<div
							class="bar-fill"
							style="width: {(renderedCount / Math.max(1, results.length)) *
								100}%"
						></div>
					</div>
				</div>
			{/if}
			{#if error}
				<div class="error">{error}</div>
			{/if}
			{#if !busy && statusMsg}
				<div class="info">{statusMsg}</div>
			{/if}
		</section>
	</aside>

	<main class="results">
		{#if results.length === 0 && !busy && lastSearch}
			<div class="empty">
				<h2>{lastSearch.cancelled ? 'Cancelled before any matches' : 'No matches'}</h2>
				<p>
					Checked {lastSearch.attempts.toLocaleString()} seeds{lastSearch.cancelled
						? ''
						: ` (the cap was ${lastSearch.maxAttempts.toLocaleString()})`} and none passed
					every filter.
				</p>
				<ul>
					{#if lastSearch.timedOut > 0}
						<li>
							{lastSearch.timedOut.toLocaleString()} of them hit the layout timeout before
							they could be checked. Raise the timeout (or turn it off) to give dense
							seeds more time.
						</li>
					{/if}
					<li>Loosen or clear the narrowest filter, then try again.</li>
					<li>Raise Max attempts to search longer.</li>
				</ul>
			</div>
		{:else if results.length === 0 && !busy}
			<div class="empty">
				<h2>Nothing yet</h2>
				<p>
					Set your ETH address, pick any trait filters (or leave them as "Any"),
					and hit Generate.
				</p>
				<ul>
					<li>
						Every result will have the trait selections you picked; the 6-byte
						nonce gives 2<sup>48</sup> variations per trait combo.
					</li>
				</ul>
			</div>
		{:else}
			<div class="results-header">
				<div class="results-tools">
					{#if anyStats}
						<label>
							Sort
							<select bind:value={sortBy}>
								<option value="found">as found</option>
								<option value="pointsDesc">most points</option>
								<option value="pointsAsc">fewest points</option>
								<option value="colorsDesc">most colors</option>
							</select>
						</label>
					{/if}
					<label class="checkbox">
						<input type="checkbox" bind:checked={favoritesOnly} />
						<span>♥ only</span>
					</label>
				</div>
				<div class="results-summary">
					{results.length}
					{results.length === 1 ? 'seed' : 'seeds'}
					{#if renderedCount >= results.length && renderRunDuration !== null}
						<span class="muted">
							· all rendered in {(renderRunDuration / 1000).toFixed(1)}s
						</span>
					{/if}
				</div>
			</div>
			<div class="grid-results" style="--tile-size: {tileDisplayWidth}px">
				{#each displayed as c (c.seed)}
					<div class="tile-slot" class:hidden={favoritesOnly && !favorites.has(c.seed)}>
						<Tile
							candidate={c}
							{thumbWidth}
							favorited={favorites.has(c.seed)}
							onOpen={(cc) => (openDetail = cc)}
							onRendered={() => onTileRendered(c.seed)}
							onToggleFavorite={() => toggleFavorite(c.seed)}
						/>
					</div>
				{/each}
			</div>
		{/if}
	</main>

	{#if openDetail}
		{#key openDetail.seed}
		<Detail
			candidate={openDetail}
			position={navIndex >= 0 ? `${navIndex + 1} / ${navList.length}` : null}
			onPrev={navIndex > 0 ? () => stepDetail(-1) : undefined}
			onNext={navIndex >= 0 && navIndex < navList.length - 1 ? () => stepDetail(1) : undefined}
			favorited={favorites.has(openDetail.seed)}
			onClose={() => (openDetail = null)}
			onToggleFavorite={() => toggleFavorite(openDetail!.seed)}
		/>
		{/key}
	{/if}
</div>

<!-- Filter help content. Each snippet renders a FilterInfo "i" button plus the
     popup body for one filter; keeping them together makes the copy easy to
     review and extend as more filters get explanations. -->
{#snippet spiralCenterInfo()}
	<FilterInfo title="Spiral center">
		<p>
			<strong>Where the swirl is centered.</strong> Radial flow fields (Explosive, Spiral,
			Circular, RandomRadial) curl every dot-trail around one point. This filter sets where
			that point is allowed to be, as a percentage of the canvas: 0% is the left or top edge,
			100% the right or bottom edge.
		</p>
		<p>
			<strong>The center is often off the canvas.</strong> It can land anywhere from −67% to
			167%, so many pieces show only the outer arc of a swirl. Set both sliders to
			0–100% (<em>on canvas</em>) to see the eye of the spiral, or narrow them to the
			<em>middle</em> third for a centered swirl.
		</p>
		<p>
			Most centers snap to a third or a half of the canvas (0%, 33%, 50%, 67%, 100%, and so
			on), and about one in six lands at a random spot on the canvas. The map shows the
			canvas as the inner rectangle and your selection in green.
		</p>
		<p>
			What the center looks like depends on the flow. On a Spiral or Circular piece it's the
			eye of the swirl; on Explosive it's the point everything bursts out from (or rushes
			into). This is the flow field's center, not the Orbital structure's center. The two are
			picked separately.
		</p>
		<p>
			<strong>Watch out with Flow field = Any:</strong> linear pieces have no center, so
			setting this filter at all leaves them out. Turbulence also bends the trails, so on
			High turbulence the visible swirl can drift away from this point.
		</p>
		<div class="tip">
			<strong>Fast to search.</strong> The center is known before any dots are laid out, so
			even a tight range skips non-matching seeds almost instantly.
		</div>
	</FilterInfo>
{/snippet}

{#snippet flowAngleInfo()}
	<FilterInfo title="Flow angle">
		<p>
			<strong>Which way the piece's lines run.</strong> A linear flow field pushes every
			dot-trail along one shared direction. This slider sets the range of that direction,
			from <strong>0° (horizontal)</strong> through <strong>45° (diagonal)</strong> to
			<strong>90° (vertical)</strong>.
		</p>
		<p>
			<strong>Most useful with Flow field = RandomLinear.</strong> Horizontal, Diagonal and
			Vertical are locked to exactly 0°, 45° and 90°, so the trait itself already tells you
			their angle — there's nothing left to narrow. RandomLinear lands on any angle in
			between, so this is the only way to reach something like "near-vertical, but tilted".
		</p>
		<p>
			Radial flows (Explosive, Spiral, Circular, RandomRadial) curve around a centre and
			have no single angle, so the slider greys out when you pin one.
		</p>
		<p>
			<strong>Watch out with Flow field = Any:</strong> the slider stays active, and because
			radial pieces have no angle to test, narrowing the range at all silently excludes
			<em>every</em> radial seed. Leave it at the full 0–90° if you want radials in the mix.
		</p>
		<p>
			Note this is the flow field's <em>underlying</em> angle. Turbulence bends the actual
			lines away from it, so on High turbulence the visible streaks can wander well off the
			nominal angle.
		</p>
		<div class="tip">
			<strong>Hunting recursions?</strong> Set Flow field to RandomLinear and narrow the
			angle to 43–47°. Recursions turn up outside that band too, but they're far more
			common inside it.
		</div>
	</FilterInfo>
{/snippet}

{#snippet formationInfo()}
	<FilterInfo title="Formation structure">
		<p>
			<strong>Formation pieces are built as a grid of rectangular chunks.</strong> Each chunk
			is filled with its own dense lattice of dots, and the colour can switch at chunk
			borders — so the piece reads as a patchwork rather than one continuous field.
		</p>
		<p>
			The grid is laid out <em>before</em> the flow field bends it, so on turbulent pieces
			the seams turn wavy rather than straight.
		</p>
		<p>
			These filters only match Formation pieces, so they grey out when you pin a different
			Structure.
		</p>
	</FilterInfo>
{/snippet}

{#snippet formationSectionsInfo()}
	<FilterInfo title="Horizontal & Vertical sections">
		<p>
			<strong>How many columns and rows the grid is divided into.</strong> Horizontal =
			columns, Vertical = rows. Each is picked independently from
			<strong>1, 2, 3, 4, 5 or 7</strong> — never 6.
		</p>
		<p>
			A 1×1 piece is one seamless field. Higher counts give a visible patchwork, with more
			seams where the colour and the dot lattice can change.
		</p>
		<p>
			<strong>Low counts dominate.</strong> Horizontal is 1 about 47% of the time, Vertical
			about 33%; a 7 on either axis turns up only ~3–4% of the time. Demanding high counts
			on both axes gets rare fast — 7×7 is roughly 1 seed in 800, so expect a lot of
			attempts.
		</p>
		<p>
			The grid overshoots the canvas on every side, so the outermost chunks are partly
			cropped — a "3 column" piece shows three columns' worth of structure, but the outer
			two bleed off the edges.
		</p>
	</FilterInfo>
{/snippet}

{#snippet formationSpacingInfo()}
	<FilterInfo title="Point spacing (Formation)">
		<p>
			<strong>How tightly the dots are packed inside each chunk</strong> — the gap between
			neighbouring points, as a percentage of canvas width. Small values give a dense,
			carpet-like fill; large values give a sparse scatter.
		</p>
		<p>
			Checking several boxes matches seeds using <em>any</em> of those values. Leaving them
			all unchecked means no constraint.
		</p>
		<p>
			<strong>The sparse values are rare:</strong> 0.75% and 1% together account for about
			three quarters of seeds, 2% for a quarter, while 4% and 8% are roughly 2% and 1%
			respectively.
		</p>
	</FilterInfo>
{/snippet}

{#snippet formationSkipInfo()}
	<FilterInfo title="Skip odds">
		<p>
			<strong>The "missing pieces" dial.</strong> Once the grid is laid out, every chunk
			except the first is deleted with this probability. 0 keeps the full grid; 0.5 drops
			about half of them, leaving large empty gaps.
		</p>
		<p>
			This is the seed's <em>dice weighting</em>, not the outcome — a piece with 0.5 skip
			odds might still keep most of its chunks by luck. If you care about the result rather
			than the setting, filter <strong>Actual chunks</strong> instead.
		</p>
		<p>
			About half of all Formation pieces have no skipping at all (0), with 0.1 next most
			common and 0.5 the rarest at roughly 5%.
		</p>
	</FilterInfo>
{/snippet}

{#snippet formationChunksInfo()}
	<FilterInfo title="Actual chunks">
		<p>
			<strong>How many chunks actually got placed</strong> — the Horizontal × Vertical grid
			minus whatever Skip odds deleted. A piece always keeps at least one, and the most
			possible is 49 (a full 7×7 grid).
		</p>
		<p>
			This is the ground-truth count, so it's usually the better filter when you care how
			much structure survived. Setting a minimum of 2 guarantees at least one visible seam.
		</p>
		<p>
			It interacts with the section limits: capping Horizontal and Vertical also caps how
			many chunks can exist. If you ask for more than the grid can hold, the app will warn
			you rather than search for something impossible.
		</p>
	</FilterInfo>
{/snippet}

{#snippet orbitalInfo()}
	<FilterInfo title="Orbital structure">
		<p>
			<strong>Orbital pieces are built as concentric bands around a centre point.</strong>
			Bands stack outward, all the same thickness, until the piece covers the canvas — so
			the band count falls out of the geometry rather than being chosen directly.
		</p>
		<p>
			The centre can sit off the canvas entirely, in which case you see huge arcs sweeping
			in from one side instead of a bullseye.
		</p>
		<p>
			Everything here describes the piece <em>before</em> the flow field and turbulence
			reshape it. On a high-turbulence seed the rings can be stretched into streaks and be
			hard to count by eye.
		</p>
	</FilterInfo>
{/snippet}

{#snippet ringBandsInfo()}
	<FilterInfo title="Ring bands">
		<p>
			<strong>How many concentric bands the piece is made of.</strong> Each band is a ring
			of the thickness set by Band thickness, and they keep stacking outward until the piece
			covers the canvas.
		</p>
		<p>
			Because of that, the count is a <em>consequence</em> of two other things: thick bands
			and a centred origin give as few as 3–6, while thin bands and a far off-canvas centre
			can produce 60 or more.
		</p>
		<p>
			A high band count therefore tends to mean fine, closely-spaced rings — often with the
			centre off the canvas.
		</p>
	</FilterInfo>
{/snippet}

{#snippet allowedSplitsInfo()}
	<FilterInfo title="Allowed splits">
		<p>
			Every band independently breaks into <strong>1</strong> (a continuous ring),
			<strong>2</strong> (halves) or <strong>3</strong> (thirds) wedge arcs.
		</p>
		<p>
			<strong>This is a whitelist:</strong> a seed only matches if <em>every</em> band uses
			one of the values you check. Check only 1 to demand all-continuous rings; check 1 and
			2 to allow halves but ban thirds. Leaving everything unchecked means no constraint.
		</p>
		<p>
			Checking a value doesn't force it to appear — it only permits it. To require one, use
			<strong>Required splits</strong>.
		</p>
		<div class="tip">
			<strong>Splits are often invisible.</strong> A wedge boundary only shows up if the
			colour happens to change there, which is a dice roll weighted by Color variety — at
			Low it fires roughly one time in nine. So a piece can be full of splits you simply
			can't see. Pair these filters with High color variety if you want them visible.
		</div>
	</FilterInfo>
{/snippet}

{#snippet requiredSplitsInfo()}
	<FilterInfo title="Required splits">
		<p>
			<strong>The opposite of Allowed splits.</strong> Each value you check must appear in
			at least one band. It says nothing about the other bands — they can be anything.
		</p>
		<p>
			Requiring 3 means "somewhere in this piece, a band is split into thirds". Requiring 2
			and 3 means both must appear <em>somewhere</em> — it is an "and", not an "or", so
			there's no way to ask for "either a 2 or a 3".
		</p>
		<p>
			The two filters combine: Allowed 1 + 2 with Required 2 means no band may be a 3, and
			at least one must be a 2.
		</p>
		<div class="tip">
			<strong>A weaker lever than it looks.</strong> This guarantees a split band
			<em>exists</em>, not that you'll be able to see it — see the note under Allowed
			splits about colour changes.
		</div>
	</FilterInfo>
{/snippet}

{#snippet orbitalCenterInfo()}
	<FilterInfo title="Center X & Center Y">
		<p>
			<strong>Where the rings radiate from.</strong> Each axis is picked from a handful of
			fixed positions:
		</p>
		<p>The boxes run in canvas order, left to right (X) or top to bottom (Y):</p>
		<ul>
			<li><strong>far</strong> — well outside, about 1.6 canvases away (off the left / top, or the right / bottom)</li>
			<li><strong>out</strong> — just past the edge, a third of a canvas outside</li>
			<li><strong>⅓ / ⅔</strong> — on the third lines (33% or 67%)</li>
			<li><strong>½</strong> — dead centre on that axis</li>
		</ul>
		<p>
			Each side is its own box, so you can pick exactly where the rings come from. For
			example X <strong>out →</strong> with Y <strong>out ↓</strong> gives rings sweeping in
			from the bottom-right corner. Hover a box for its exact position.
		</p>
		<p>
			Both axes centred gives a symmetric bullseye. Push one axis off-canvas and you get
			giant arcs sweeping in from that side with no visible centre at all — a completely
			different look, and the main reason two Orbital pieces can seem unrelated.
		</p>
		<p>
			The centre position also drives how many Ring bands fit: the further off-canvas it
			sits, the more bands are needed to cover the piece.
		</p>
		<p>
			Checking several boxes matches any of them; dead centre is the single most common
			value on each axis, and the far-outside positions are the rarest.
		</p>
	</FilterInfo>
{/snippet}

{#snippet orbitalSpacingInfo()}
	<FilterInfo title="Point spacing (Orbital)">
		<p>
			<strong>The gap between dots travelling around each ring</strong>, as a percentage of
			canvas width. The spacing between rings within a band is derived from it automatically
			— half this value.
		</p>
		<p>
			1–2% reads as smooth, continuous beaded rings. 8–16% gives loose dotted orbits with
			visible gaps between the points.
		</p>
		<p>
			The tight values are much more common than the loose ones — 1% alone is about a third
			of seeds, while 16% is only around 6%.
		</p>
	</FilterInfo>
{/snippet}

{#snippet bandThicknessInfo()}
	<FilterInfo title="Band thickness">
		<p>
			<strong>How thick each concentric band is</strong>, as a percentage of canvas width.
			Only three values occur — <strong>7%</strong>, <strong>15%</strong> and
			<strong>30%</strong> — and they're equally likely.
		</p>
		<p>
			This is the main driver of the piece's character: 7% gives many fine rings, 30% gives
			a few bold, chunky ones. Together with the centre position it also determines how many
			<strong>Ring bands</strong> the piece ends up with.
		</p>
	</FilterInfo>
{/snippet}

{#snippet splitOffsetInfo()}
	<FilterInfo title="Split offset">
		<p>
			<strong>Where around the circle the wedge seams start.</strong> A single random
			rotation from 0–360°, shared by every band in the piece.
		</p>
		<p>
			It rotates all the split boundaries together and changes nothing else — no effect on
			band count, spacing or colour. Worth filtering only if you're hunting a specific seam
			placement, such as a boundary running through the top of the piece.
		</p>
		<p>
			On a piece whose bands are all continuous (1 split), it has no visible effect at all.
		</p>
	</FilterInfo>
{/snippet}

{#snippet shadowsInfo()}
	<FilterInfo title="Shadows structure">
		<p>
			<strong>Shadows pieces are made of scattered, non-overlapping circles.</strong> Each
			circle is dropped at a random spot with a random size, and then filled — either with
			concentric rings or with a square grid of dots.
		</p>
		<p>
			Circles are placed one at a time and any that would overlap an existing circle is
			thrown away, so the piece reads as separate islands of texture on open ground.
		</p>
		<p>
			These filters only match Shadows pieces, so they grey out when you pin a different
			Structure.
		</p>
	</FilterInfo>
{/snippet}

{#snippet shadowsTargetInfo()}
	<FilterInfo title="Target circles">
		<p>
			<strong>How many circles the algorithm sets out to place</strong> — one of 5, 7, 10,
			20, 30 or 60, all equally likely.
		</p>
		<p>
			This is an <em>intention</em>, not a result. Circles are sized anywhere from 5% to 50%
			of the canvas, so on the crowded targets most attempts collide with a circle that's
			already there and get discarded. A target of 60 rarely yields anything close to 60.
		</p>
		<p>
			If you care about what you can actually count in the image, filter
			<strong>Actual circles</strong> instead.
		</p>
	</FilterInfo>
{/snippet}

{#snippet shadowsActualInfo()}
	<FilterInfo title="Actual circles">
		<p>
			<strong>How many circles actually landed</strong> after collisions were rejected —
			the ground-truth count, and the number you can count in the finished piece.
		</p>
		<p>
			Placement gives up after a thousand failed attempts, so this can fall well short of
			the target. Small targets nearly always fit completely; large ones almost never do.
		</p>
		<p>
			A low count means a few big circles with lots of open space; a high count means many
			small ones packed together.
		</p>
	</FilterInfo>
{/snippet}

{#snippet shadowsFillInfo()}
	<FilterInfo title="Fill style">
		<p>
			<strong>What each circle is filled with</strong> — either concentric rings
			(<em>radial</em>) or a square grid of dots (<em>square</em>). The piece follows one of
			three rules, all equally likely:
		</p>
		<ul>
			<li><strong>All radial</strong> — every circle is concentric rings</li>
			<li><strong>Mixed</strong> — each circle flips its own 50/50 coin</li>
			<li><strong>All square</strong> — every circle is a grid</li>
		</ul>
		<p>
			Mixed is the only setting where both textures appear in the same piece, so it's the
			one to choose if you want that contrast. The two "all" settings give a more uniform,
			consistent look.
		</p>
	</FilterInfo>
{/snippet}

{#snippet shadowsSquareDirInfo()}
	<FilterInfo title="Square direction">
		<p>
			For circles filled with a square grid, whether the dots are laid down
			<strong>column by column</strong> or <strong>row by row</strong>.
		</p>
		<p>
			The dots end up in exactly the same places either way — only the <em>order</em> they
			are drawn in changes. Since colour drifts gradually as dots are placed, that order
			nudges whether the colour gradient runs across the grid or down it.
		</p>
		<p>
			Genuinely subtle, and it does nothing at all on a piece with no square-filled circles.
		</p>
	</FilterInfo>
{/snippet}

{#snippet shadowsRadialDirInfo()}
	<FilterInfo title="Radial direction">
		<p>
			For circles filled with concentric rings, whether they're drawn
			<strong>outside-in</strong> or <strong>inside-out</strong>.
		</p>
		<p>
			As with Square direction, the dots land in the same places — the order decides which
			rings overlap which, and which way the colour drifts across the circle.
		</p>
		<p>
			Subtle, and worth knowing that a small fraction of radial circles have their drawing
			order shuffled at random regardless of this setting, which scrambles the layering
			anyway.
		</p>
	</FilterInfo>
{/snippet}

<style>
	:global(:root) {
		color-scheme: dark;
		font-family:
			'Inter',
			system-ui,
			-apple-system,
			sans-serif;
		font-size: 14px;
		color: #ddd;
		background: #0d0d0d;
	}
	:global(html, body) {
		margin: 0;
		padding: 0;
		height: 100%;
		background: #0d0d0d;
	}
	:global(*) {
		box-sizing: border-box;
	}
	.app {
		display: grid;
		grid-template-columns: 340px 1fr;
		height: 100vh;
		overflow: hidden;
	}
	.controls {
		background: #161616;
		border-right: 1px solid #222;
		padding: 18px 18px 24px;
		overflow-y: auto;
		display: flex;
		flex-direction: column;
		gap: 18px;
	}
	header h1 {
		margin: 0;
		font-size: 18px;
		letter-spacing: 0.02em;
	}
	.sub {
		margin: 2px 0 0;
		color: #666;
		font-size: 12px;
	}
	section {
		display: flex;
		flex-direction: column;
		gap: 8px;
	}
	section.group {
		padding: 12px 14px 14px;
		border-radius: 8px;
		border-left: 3px solid #444;
		background: #131313;
	}
	section.group-setup {
		background: #14171f;
		border-left-color: #4f6ee0;
	}
	section.group-traits {
		background: #131a14;
		border-left-color: #4caf6a;
	}
	section.group-layout {
		background: #1a141e;
		border-left-color: #a66bd1;
	}
	section.group-composition {
		background: #121b1d;
		border-left-color: #3cb6c4;
	}
	section.group-colors {
		background: #1e141a;
		border-left-color: #e16a99;
	}
	section.group-generation {
		background: #1e1a14;
		border-left-color: #e0a14f;
	}
	.section-head {
		display: flex;
		align-items: baseline;
		justify-content: space-between;
	}
	.section-head h2 {
		margin: 0;
		font-size: 11px;
		text-transform: uppercase;
		color: #888;
		letter-spacing: 0.08em;
	}
	.hint {
		color: #555;
		font-size: 11px;
	}
	.hint.fast {
		color: #4cb088;
	}
	.label {
		display: block;
		font-size: 11px;
		text-transform: uppercase;
		color: #888;
		letter-spacing: 0.05em;
		margin-bottom: 3px;
	}
	.block {
		display: block;
	}
	/* Scoped to Setup only — these blocks stack vertically and need spacing.
	   Elsewhere `.block` lives inside grids that handle their own gaps, so a
	   global rule here would push the 2nd item of every grid row down. */
	.group-setup .block + .block {
		margin-top: 10px;
	}
	.folder-row {
		display: flex;
		align-items: center;
		gap: 8px;
	}
	.folder-row .ghost {
		flex-shrink: 0;
	}
	.folder-path {
		font-size: 11px;
		color: #aaa;
		font-family: ui-monospace, SFMono-Regular, monospace;
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
		direction: rtl;
		text-align: left;
	}
	.folder-path.none {
		color: #666;
		font-style: italic;
		direction: ltr;
	}
	.inspect-err {
		color: #d66;
		font-size: 11px;
		margin-top: 4px;
		font-family: ui-monospace, monospace;
	}
	.grid {
		display: grid;
		grid-template-columns: 1fr 1fr;
		gap: 8px;
	}
	.char-grid {
		display: grid;
		grid-template-columns: 1fr 1fr;
		gap: 8px;
	}
	.bucket-row {
		display: grid;
		grid-template-columns: 1fr 1fr 1fr;
		gap: 6px;
		align-items: center;
		margin-bottom: 4px;
	}
	.workers-row {
		margin-top: 8px;
	}
	.workers-head {
		display: flex;
		justify-content: space-between;
		align-items: baseline;
	}
	.workers-value {
		font-family: ui-monospace, monospace;
		font-variant-numeric: tabular-nums;
		font-size: 13px;
		color: #fff;
	}
	.workers-row input[type='range'] {
		width: 100%;
		padding: 0;
		background: transparent;
		border: 0;
	}
	.workers-hint {
		font-size: 11px;
		color: #777;
		margin-top: 2px;
	}
	.timeout-row {
		margin-top: 6px;
	}
	.timeout-hint {
		font-size: 11px;
		color: #777;
		margin-top: 2px;
	}
	.bucket-label {
		display: flex;
		flex-direction: column;
		gap: 0;
	}
	.bucket-label strong {
		font-size: 12px;
		font-weight: 500;
		color: #ddd;
	}
	.bucket-label small {
		font-size: 10px;
		color: #666;
	}
	.splits-row {
		display: grid;
		grid-template-columns: 2fr 1fr;
		gap: 6px;
		align-items: center;
		margin-bottom: 4px;
	}
	.splits-label {
		display: flex;
		flex-direction: column;
		gap: 0;
	}
	.splits-label strong {
		font-size: 12px;
		font-weight: 500;
		color: #ddd;
	}
	.splits-label small {
		font-size: 10px;
		color: #666;
	}
	.center-pos {
		margin: 6px 0 4px;
	}
	.center-pos-grid {
		display: grid;
		grid-template-columns: repeat(7, 1fr);
		margin-top: 4px;
		border-radius: 4px;
		background: linear-gradient(
			90deg,
			transparent 28.57%,
			rgba(255, 255, 255, 0.05) 28.57%,
			rgba(255, 255, 255, 0.05) 71.43%,
			transparent 71.43%
		);
	}
	.center-pos-cell {
		display: flex;
		flex-direction: column;
		align-items: center;
		gap: 2px;
		padding: 3px 0;
		font-size: 12px;
		color: #999;
		cursor: pointer;
	}
	.center-pos-cell.onCanvas {
		color: #ddd;
	}
	.center-pos-cell input {
		margin: 0;
	}
	.center-pos-legend {
		display: flex;
		justify-content: space-between;
		font-size: 10px;
		color: #666;
		margin-top: 1px;
	}
	.splits-checks {
		display: flex;
		gap: 6px;
		justify-content: flex-end;
	}
	.splits-check {
		display: flex;
		align-items: center;
		gap: 3px;
		font-family: ui-monospace, monospace;
		font-size: 12px;
		color: #ddd;
		cursor: pointer;
	}
	.splits-check input[type='checkbox'] {
		width: auto;
		flex-shrink: 0;
		margin: 0;
	}
	.angle-head {
		display: flex;
		justify-content: space-between;
		align-items: baseline;
		margin-bottom: 4px;
	}
	.angle-readout {
		font-family: ui-monospace, monospace;
		font-variant-numeric: tabular-nums;
		font-size: 13px;
		color: #fff;
	}
	.dual-range {
		position: relative;
		height: 28px;
	}
	.dual-range .track {
		position: absolute;
		top: 13px;
		left: 0;
		right: 0;
		height: 4px;
		background: #2a2a2a;
		border-radius: 2px;
	}
	.dual-range .track-fill {
		position: absolute;
		top: 13px;
		height: 4px;
		background: linear-gradient(90deg, #3a8b66, #4cb088);
		border-radius: 2px;
	}
	.dual-range input[type='range'] {
		position: absolute;
		top: 0;
		left: 0;
		width: 100%;
		height: 28px;
		background: transparent;
		appearance: none;
		-webkit-appearance: none;
		pointer-events: none;
		padding: 0;
		border: 0;
	}
	/* The max handle is stacked above the min handle. When both meet in the
	   right half, raise the min one — otherwise the max handle can't move left
	   (it can't pass min) and the min handle is unreachable underneath it. */
	.dual-range input[type='range'].raised {
		z-index: 1;
	}
	.dual-range input[type='range']::-webkit-slider-runnable-track {
		background: transparent;
		border: 0;
		height: 28px;
	}
	.dual-range input[type='range']::-webkit-slider-thumb {
		pointer-events: auto;
		-webkit-appearance: none;
		appearance: none;
		width: 16px;
		height: 16px;
		border-radius: 50%;
		background: #ddd;
		border: 2px solid #1a1a1a;
		margin-top: 0;
		cursor: pointer;
		box-shadow: 0 1px 3px rgba(0, 0, 0, 0.4);
	}
	.center-picker {
		display: flex;
		gap: 10px;
		align-items: center;
	}
	.center-map {
		width: 56px;
		height: 70px;
		flex-shrink: 0;
	}
	.center-map .map-bg {
		fill: #1c1c1c;
	}
	.center-map .map-sel {
		fill: rgba(76, 176, 136, 0.45);
		stroke: #4cb088;
		stroke-width: 0.6;
	}
	.center-map .map-canvas {
		fill: none;
		stroke: #ddd;
		stroke-width: 0.8;
	}
	.center-sliders {
		flex: 1;
		min-width: 0;
	}
	.center-axis {
		font-size: 11px;
		color: #888;
	}
	/* Marks the on-canvas stretch (0–100%) of a spiral center slider. */
	.dual-range .track-canvas {
		position: absolute;
		top: 11px;
		height: 8px;
		border-left: 1px solid #666;
		border-right: 1px solid #666;
		background: #353535;
	}
	.center-presets {
		display: flex;
		gap: 6px;
		margin-top: 4px;
	}
	.angle-legend {
		display: flex;
		justify-content: space-between;
		font-size: 10px;
		color: #666;
		margin-top: 2px;
	}
	.subhead {
		font-size: 11px;
		text-transform: uppercase;
		letter-spacing: 0.05em;
		color: #888;
		margin: 12px 0 4px;
	}
	.subhead small {
		text-transform: none;
		color: #555;
		font-size: 10px;
		margin-left: 6px;
		letter-spacing: 0;
	}
	.subhead small.locked {
		color: #c08a5a;
	}
	/* Structure-specific filter groups. Reset the native fieldset chrome so it
	   lays out like a plain block; dim + disable when its Structure isn't chosen. */
	fieldset.struct-group {
		border: 0;
		margin: 0;
		padding: 0;
		min-width: 0;
	}
	fieldset.struct-group:disabled {
		opacity: 0.4;
	}
	/* Each structure is its own top-level card, styled to match section.group
	   (they're fieldsets, not sections, so the whole card — edge included —
	   dims via :disabled when its Structure isn't the chosen one). */
	fieldset.struct-card {
		border-left: 3px solid #444;
		border-radius: 8px;
		padding: 12px 14px 14px;
	}
	fieldset.struct-formation {
		background: #1a1812;
		border-left-color: #d1b13c;
	}
	fieldset.struct-orbital {
		background: #12161e;
		border-left-color: #5a8de0;
	}
	fieldset.struct-shadows {
		background: #15181b;
		border-left-color: #7f96ad;
	}
	.swatch {
		display: inline-block;
		width: 14px;
		height: 14px;
		border-radius: 3px;
		border: 1px solid #333;
		flex-shrink: 0;
	}
	.color-chips-grid {
		display: grid;
		grid-template-columns: repeat(2, 1fr);
		gap: 4px;
		margin-top: 4px;
	}
	.chip-button {
		display: flex;
		align-items: center;
		gap: 6px;
		padding: 4px 8px;
		background: #0d0d0d;
		border: 1px solid #2a2a2a;
		border-radius: 4px;
		color: #aaa;
		font-size: 11px;
		cursor: pointer;
		text-align: left;
	}
	.chip-button:hover {
		border-color: #4a4;
	}
	.chip-button.selected {
		background: #1d3a2a;
		border-color: #3a8b66;
		color: #fff;
	}
	.chip-odds {
		font-size: 10px;
		color: #777;
		font-variant-numeric: tabular-nums;
	}
	.chip-button.selected .chip-odds {
		color: #9fd4bb;
	}
	.bg-group-label {
		font-size: 10px;
		color: #777;
		margin: 8px 0 0;
	}
	.bg-hint {
		display: block;
		font-size: 10px;
		color: #555;
		margin-top: 4px;
	}
	.chip-name {
		flex: 1;
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.checkbox {
		display: flex;
		align-items: center;
		gap: 6px;
		color: #aaa;
		font-size: 12px;
		margin-top: 4px;
	}
	.checkbox input[type='checkbox'] {
		width: auto;
		flex-shrink: 0;
	}
	.bullseye {
		margin-top: 8px;
	}
	.bullseye-row {
		display: grid;
		grid-template-columns: 1fr 1fr 1fr 1fr;
		gap: 6px;
	}
	.bull {
		display: flex;
		flex-direction: column;
		gap: 3px;
		color: #aaa;
		font-size: 11px;
		text-align: center;
	}
	.bull span {
		font-family: ui-monospace, monospace;
	}
	input,
	select {
		width: 100%;
		background: #0d0d0d;
		color: #eee;
		border: 1px solid #2a2a2a;
		border-radius: 4px;
		padding: 6px 8px;
		font-size: 13px;
		font-family: inherit;
	}
	input:focus,
	select:focus {
		outline: 1px solid #4a8;
		border-color: #4a8;
	}
	button {
		background: #2a2a2a;
		color: #ddd;
		border: 1px solid #3a3a3a;
		border-radius: 4px;
		padding: 6px 12px;
		cursor: pointer;
		font-family: inherit;
		font-size: 13px;
	}
	button:hover:not(:disabled) {
		background: #3a3a3a;
	}
	button:disabled {
		opacity: 0.4;
		cursor: default;
	}
	button.ghost {
		background: transparent;
		border: 0;
		color: #888;
		font-size: 11px;
		padding: 0;
	}
	button.ghost:hover {
		color: #ddd;
	}
	button.primary {
		background: #2d6a4f;
		border-color: #3a8b66;
		font-weight: 500;
		width: 100%;
		padding: 10px;
		margin-top: 8px;
	}
	button.primary:hover:not(:disabled) {
		background: #3a8b66;
	}
	button.cancel {
		background: #5a2a2a;
		border-color: #7a3a3a;
		font-weight: 500;
		width: 100%;
		padding: 10px;
		margin-top: 8px;
		color: #f5d5d5;
	}
	button.cancel:hover {
		background: #7a3a3a;
	}
	.progress {
		margin-top: 10px;
		display: flex;
		flex-direction: column;
		gap: 5px;
		font-size: 12px;
		color: #ccc;
	}
	.progress-line {
		display: flex;
		flex-wrap: wrap;
		gap: 6px;
		align-items: baseline;
	}
	.progress-line strong {
		color: #fff;
		font-variant-numeric: tabular-nums;
	}
	.progress-line .phase {
		text-transform: uppercase;
		letter-spacing: 0.05em;
		font-size: 10px;
		color: #888;
		margin-right: 4px;
	}
	.bar {
		height: 4px;
		background: #2a2a2a;
		border-radius: 2px;
		overflow: hidden;
	}
	.bar-fill {
		height: 100%;
		background: linear-gradient(90deg, #3a8b66, #4cb088);
		transition: width 0.1s linear;
	}
	.error {
		color: #d66;
		font-size: 12px;
		margin-top: 4px;
		padding: 6px 8px;
		background: rgba(220, 80, 80, 0.1);
		border-radius: 4px;
		white-space: pre-line;
	}
	/* Shown when the chosen Flow field pins the angle, so the range slider
	   can't actually narrow anything. */
	.angle-note.warn {
		color: #c08a5a;
		background: rgba(192, 138, 90, 0.12);
	}
	.angle-note {
		color: #8fa3b8;
		font-size: 11px;
		margin-top: 6px;
		padding: 5px 7px;
		background: rgba(143, 163, 184, 0.1);
		border-radius: 4px;
		line-height: 1.4;
	}
	.combo-warn {
		color: #c08a5a;
		font-size: 11px;
		margin-top: 4px;
		padding: 5px 7px;
		background: rgba(192, 138, 90, 0.12);
		border-radius: 4px;
		line-height: 1.4;
	}
	.info {
		color: #aaa;
		font-size: 12px;
		margin-top: 4px;
	}
	.results {
		overflow-y: auto;
		padding: 18px;
	}
	.empty {
		max-width: 520px;
		margin: 60px auto;
		color: #aaa;
	}
	.empty h2 {
		color: #ddd;
		font-weight: 500;
	}
	.empty ul {
		padding-left: 18px;
	}
	.empty li {
		margin-bottom: 8px;
	}
	.grid-results {
		display: grid;
		grid-template-columns: repeat(auto-fill, minmax(var(--tile-size, 240px), 1fr));
		gap: 12px;
	}
	.filters-bar {
		display: flex;
		align-items: baseline;
		justify-content: space-between;
		margin: 4px 2px 6px;
		font-size: 11px;
		text-transform: uppercase;
		letter-spacing: 0.05em;
		color: #888;
	}
	.filters-bar small {
		text-transform: none;
		letter-spacing: 0;
		color: #555;
		margin-left: 4px;
	}
	.tile-slot.hidden {
		display: none;
	}
	.results-tools {
		display: flex;
		align-items: center;
		gap: 12px;
		font-size: 12px;
		color: #aaa;
	}
	.results-tools select {
		margin-left: 4px;
	}
	.count-warn {
		display: block;
		font-size: 10px;
		color: #c08a5a;
		margin-top: 3px;
		line-height: 1.3;
	}
	.progress-line.sub {
		font-size: 11px;
		color: #999;
		margin-top: 2px;
	}
	.results-header {
		display: flex;
		align-items: center;
		justify-content: space-between;
		margin-bottom: 10px;
		font-size: 13px;
		color: #ddd;
	}
	.results-summary .muted {
		color: #777;
		margin-left: 4px;
	}
</style>

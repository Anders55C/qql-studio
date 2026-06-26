<script lang="ts">
	import { onDestroy, onMount } from 'svelte';
	import { listen, type UnlistenFn } from '@tauri-apps/api/event';
	import {
		cancelSearch,
		decodeSeed,
		deleteSeedPng,
		emptyChoices,
		generateCandidates,
		layoutSummary,
		listPaletteColors,
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
	import Detail from '$lib/Detail.svelte';
	import { setRenderConcurrency } from '$lib/renderQueue';

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

	// When a palette is pinned, only that palette's colors are reachable.
	const visibleBackgrounds = $derived.by(() => {
		const selected = choices.colorPalette;
		const map = new Map<string, ColorInfo>();
		for (const p of paletteColors) {
			if (selected && p.palette !== selected) continue;
			for (const c of p.backgrounds) {
				map.set(c.name, c);
			}
		}
		return [...map.values()].sort((a, b) => a.name.localeCompare(b.name));
	});
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
	// When the angle filter doesn't apply (radial flow chosen), reset it to the
	// full range so it reads as "no constraint" and can't silently reject seeds.
	$effect(() => {
		if (!angleApplies) {
			minAngle = 0;
			maxAngle = 90;
		}
	});
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
	// Actual chunks can't exceed the grid, whose largest size is (max H)×(max V).
	// "no cap" on a section axis means up to 7 (the largest section value).
	const maxAchievableChunks = $derived(
		(maxHorizontalSections ?? 7) * (maxVerticalSections ?? 7)
	);
	const chunksImpossible = $derived(
		formationEnabled &&
			minFormationChunks != null &&
			minFormationChunks > maxAchievableChunks
	);
	// Provably-impossible filter combinations, surfaced inline and blocked at
	// Generate. Only deterministic impossibilities go here (not merely-rare ones).
	function impossibleReasons(): string[] {
		const reasons: string[] = [];
		if (chunksImpossible) {
			reasons.push(
				`Actual chunks: minimum is ${minFormationChunks}, but with Horizontal ≤ ${maxHorizontalSections ?? 7} and Vertical ≤ ${maxVerticalSections ?? 7}, the most chunks a Formation can have is ${maxAchievableChunks}. Lower the minimum, or raise the section limits.`
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
	// Center X category checkboxes (0=Centered, 1=Off-center, 2=Just outside, 3=Way outside).
	let cxCentered = $state<boolean>(false);
	let cxOffCenter = $state<boolean>(false);
	let cxJustOutside = $state<boolean>(false);
	let cxWayOutside = $state<boolean>(false);
	let cyCentered = $state<boolean>(false);
	let cyOffCenter = $state<boolean>(false);
	let cyJustOutside = $state<boolean>(false);
	let cyWayOutside = $state<boolean>(false);
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
	let backgroundColor = $state<string>('');
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
		} catch {}
		traitsMeta = await listTraits();
		paletteColors = await listPaletteColors();
		unlisten = await listen<GenerateProgress>('search-progress', (event) => {
			progress = event.payload;
		});
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
					minAngle,
					maxAngle,
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

	async function onGenerate() {
		error = null;
		statusMsg = '';
		// Bail before churning attempts on a provably-impossible combination.
		const reasons = impossibleReasons();
		if (reasons.length > 0) {
			error = `This combination can't produce results:\n• ${reasons.join('\n• ')}`;
			return;
		}
		progress = null;
		results = [];
		renderedCount = 0;
		renderRunStart = null;
		renderRunDuration = null;
		cancelling = false;
		busy = true;
		try {
			const charFilter: CharacteristicFilter = {
				minPoints: minPoints ?? undefined,
				maxPoints: maxPoints ?? undefined,
				minColors: minColors ?? undefined,
				maxColors: maxColors ?? undefined,
				minSmall: minSmall ?? undefined,
				maxSmall: maxSmall ?? undefined,
				minMedium: minMedium ?? undefined,
				maxMedium: maxMedium ?? undefined,
				minLarge: minLarge ?? undefined,
				maxLarge: maxLarge ?? undefined,
				cornerConcentration: cornerConcentration || undefined,
				cornerRatio: cornerConcentration ? cornerRatio : undefined,
				minHorizontalSections: minHorizontalSections ?? undefined,
				maxHorizontalSections: maxHorizontalSections ?? undefined,
				minVerticalSections: minVerticalSections ?? undefined,
				maxVerticalSections: maxVerticalSections ?? undefined,
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
				minFormationChunks: minFormationChunks ?? undefined,
				maxFormationChunks: maxFormationChunks ?? undefined,
				minRingBands: minRingBands ?? undefined,
				maxRingBands: maxRingBands ?? undefined,
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
				allowedOrbitalCenterXCategories: (() => {
					const list = [
						cxCentered ? 0 : null,
						cxOffCenter ? 1 : null,
						cxJustOutside ? 2 : null,
						cxWayOutside ? 3 : null
					].filter((v): v is number => v !== null);
					return list.length > 0 ? list : undefined;
				})(),
				allowedOrbitalCenterYCategories: (() => {
					const list = [
						cyCentered ? 0 : null,
						cyOffCenter ? 1 : null,
						cyJustOutside ? 2 : null,
						cyWayOutside ? 3 : null
					].filter((v): v is number => v !== null);
					return list.length > 0 ? list : undefined;
				})(),
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
				backgroundColor: backgroundColor || undefined,
				ringColors: ringColors.length > 0 ? ringColors : undefined,
				exactRingColors: ringColors.length > 0 ? exactRingColors : undefined,
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
				minShadowsActualCircles: minShadowsActualCircles ?? undefined,
				maxShadowsActualCircles: maxShadowsActualCircles ?? undefined,
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
				charFilter.allowedOrbitalCenterXCategories = undefined;
				charFilter.allowedOrbitalCenterYCategories = undefined;
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
			const isCharFilterActive =
				Object.values(charFilter).some((v) => v !== undefined && v !== null);
			const angleActive = angleApplies && (minAngle > 0 || maxAngle < 90);
			const outcome = await generateCandidates({
				address,
				choices,
				mode,
				count,
				maxAttempts: maxAttempts ?? undefined,
				workers: workers > 0 ? workers : undefined,
				runLayout: runLayout || isCharFilterActive,
				characteristicFilter: isCharFilterActive ? charFilter : undefined,
				minAngleDeg: angleActive ? minAngle : undefined,
				maxAngleDeg: angleActive ? maxAngle : undefined,
				layoutTimeoutMs:
					layoutTimeoutEnabled && layoutTimeoutSeconds > 0
						? Math.round(layoutTimeoutSeconds * 1000)
						: undefined
			});
			results = outcome.candidates;
			if (outcome.cancelled) {
				statusMsg = `Cancelled. Got ${results.length}/${count} after ${outcome.attempts.toLocaleString()} attempts.`;
			} else if (results.length < count) {
				statusMsg = `Got ${results.length}/${count} after ${outcome.attempts.toLocaleString()} attempts (cap ${outcome.maxAttempts.toLocaleString()}). Try widening filters or raise max attempts.`;
			} else {
				statusMsg = `Found ${results.length} in ${outcome.attempts.toLocaleString()} attempts.`;
			}
			if (results.length > 0) {
				renderRunStart = performance.now();
			}
		} catch (e) {
			error = String(e);
		} finally {
			busy = false;
			cancelling = false;
		}
	}

	async function onCancel() {
		if (cancelling) return;
		cancelling = true;
		try {
			await cancelSearch();
		} catch {}
	}

	function onTileRendered() {
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

	// Inspect any seed: decode its traits, fetch layout stats, and open the
	// Detail view (where it can be rendered, animated, and exported).
	async function inspectSeed() {
		const raw = inspectHex.trim();
		if (!raw || inspecting) return;
		inspectErr = null;
		inspecting = true;
		try {
			const traits = await decodeSeed(raw);
			const seed = raw.startsWith('0x') ? raw : `0x${raw}`;
			let stats = null;
			try {
				stats = await layoutSummary(seed);
			} catch {
				// non-fatal — Detail still renders + animates without layout stats
			}
			openDetail = { seed, traits, stats };
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
					<input type="number" bind:value={minColors} placeholder="—" min="1" max="20" />
				</label>
				<label class="block">
					<span class="label">Max colors</span>
					<input type="number" bind:value={maxColors} placeholder="—" min="1" max="20" />
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
				<small class="hint">requires layout</small>
			</div>

			<div class="subhead">Background</div>
			<label class="block">
				<select bind:value={backgroundColor}>
					<option value="">Any background</option>
					{#each visibleBackgrounds as c}
						<option value={c.name}>{c.name}</option>
					{/each}
				</select>
			</label>
			{#if backgroundColor}
				{@const swatch = visibleBackgrounds.find((c) => c.name === backgroundColor)}
				{#if swatch}
					<div class="bg-preview">
						<span class="swatch" style="background:{swatch.swatch}"></span>
						<span>{swatch.name}</span>
					</div>
				{/if}
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
			{#if ringColors.length > 0}
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

			<div class="subhead">Splatter</div>
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
				<small class="hint">requires layout</small>
			</div>

			<fieldset class="struct-group" disabled={!angleApplies}>
			<div class="subhead">
				Flow angle
				{#if angleApplies}
					<small>linear flows only</small>
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

			<fieldset class="struct-group" disabled={!formationEnabled}>
				<div class="subhead">
					Formation structure
					{#if formationEnabled}
						<small>Formation pieces only</small>
					{:else}
						<small class="locked">n/a — Structure is {choices.structure}</small>
					{/if}
				</div>
			<div class="bucket-row">
				<div class="bucket-label">
					<strong>Horizontal</strong>
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
					<strong>Vertical</strong>
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
					<strong>Point spacing</strong>
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
					<strong>Skip odds</strong>
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
					<strong>Actual chunks</strong>
					<small>placed after skips</small>
				</div>
				<input type="number" bind:value={minFormationChunks} placeholder="min" min="1" max="49" />
				<input type="number" bind:value={maxFormationChunks} placeholder="max" min="1" max="49" />
			</div>
			{#if chunksImpossible}
				<div class="combo-warn">
					⚠ With Horizontal ≤ {maxHorizontalSections ?? 7} and Vertical ≤ {maxVerticalSections ??
						7}, the most chunks possible is {maxAchievableChunks} — a minimum above that
					matches nothing.
				</div>
			{/if}

			</fieldset>

			<fieldset class="struct-group" disabled={!orbitalEnabled}>
				<div class="subhead">
					Orbital structure
					{#if orbitalEnabled}
						<small>Orbital pieces only</small>
					{:else}
						<small class="locked">n/a — Structure is {choices.structure}</small>
					{/if}
				</div>
			<div class="bucket-row">
				<div class="bucket-label">
					<strong>Ring bands</strong>
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
					<strong>Allowed splits</strong>
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
					<strong>Required splits</strong>
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
			<div class="splits-row">
				<div class="splits-label">
					<strong>Center X</strong>
					<small>horizontal position categories</small>
				</div>
				<div class="splits-checks">
					<label class="splits-check" title="x = 1/2">
						<input type="checkbox" bind:checked={cxCentered} />
						<span>½</span>
					</label>
					<label class="splits-check" title="x = 1/3 or 2/3">
						<input type="checkbox" bind:checked={cxOffCenter} />
						<span>⅓</span>
					</label>
					<label class="splits-check" title="x = -1/3 or 4/3">
						<input type="checkbox" bind:checked={cxJustOutside} />
						<span>out</span>
					</label>
					<label class="splits-check" title="x = -1.6 or 1.6">
						<input type="checkbox" bind:checked={cxWayOutside} />
						<span>far</span>
					</label>
				</div>
			</div>
			<div class="splits-row">
				<div class="splits-label">
					<strong>Center Y</strong>
					<small>vertical position categories</small>
				</div>
				<div class="splits-checks">
					<label class="splits-check" title="y = 1/2">
						<input type="checkbox" bind:checked={cyCentered} />
						<span>½</span>
					</label>
					<label class="splits-check" title="y = 1/3 or 2/3">
						<input type="checkbox" bind:checked={cyOffCenter} />
						<span>⅓</span>
					</label>
					<label class="splits-check" title="y = -1/3 or 4/3">
						<input type="checkbox" bind:checked={cyJustOutside} />
						<span>out</span>
					</label>
					<label class="splits-check" title="y = -1.6 or 1.6">
						<input type="checkbox" bind:checked={cyWayOutside} />
						<span>far</span>
					</label>
				</div>
			</div>
			<div class="splits-row">
				<div class="splits-label">
					<strong>Point spacing</strong>
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
					<strong>Band thickness</strong>
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
					<strong>Split offset</strong>
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

			<fieldset class="struct-group" disabled={!shadowsEnabled}>
				<div class="subhead">
					Shadows structure
					{#if shadowsEnabled}
						<small>Shadows pieces only</small>
					{:else}
						<small class="locked">n/a — Structure is {choices.structure}</small>
					{/if}
				</div>
			<div class="splits-row">
				<div class="splits-label">
					<strong>Target circles</strong>
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
				<span class="label">Fill style</span>
				<select bind:value={shadowsFillStyle}>
					<option value="">Any</option>
					<option value="all_radial">All radial (concentric rings)</option>
					<option value="mixed">Mixed (50/50 per circle)</option>
					<option value="all_square">All square (grids)</option>
				</select>
			</label>
			<div class="bucket-row">
				<div class="bucket-label">
					<strong>Actual circles</strong>
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
				<span class="label">Square direction <small>subtle · rarely useful</small></span>
				<select bind:value={shadowsColumnar}>
					<option value="">Any</option>
					<option value="col">Column-major</option>
					<option value="row">Row-major</option>
				</select>
			</label>
			<label class="block">
				<span class="label">Radial direction <small>subtle · rarely useful</small></span>
				<select bind:value={shadowsRadialDir}>
					<option value="">Any</option>
					<option value="out">Outside → in</option>
					<option value="in">Inside → out</option>
				</select>
			</label>
			</fieldset>
		</section>

		<section class="group group-generation">
			<div class="section-head">
				<h2>Generate</h2>
			</div>
			<div class="char-grid">
				<label class="block">
					<span class="label">Count</span>
					<input type="number" bind:value={count} min="1" max="200" />
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
						a non-match
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
			{#if !busy && renderedCount >= results.length && statusMsg}
				<div class="info">{statusMsg}</div>
			{/if}
		</section>
	</aside>

	<main class="results">
		{#if results.length === 0 && !busy}
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
				{#each results as c (c.seed)}
					<Tile
						candidate={c}
						{thumbWidth}
						favorited={favorites.has(c.seed)}
						onOpen={(cc) => (openDetail = cc)}
						onRendered={onTileRendered}
						onToggleFavorite={() => toggleFavorite(c.seed)}
					/>
				{/each}
			</div>
		{/if}
	</main>

	{#if openDetail}
		<Detail
			candidate={openDetail}
			favorited={favorites.has(openDetail.seed)}
			onClose={() => (openDetail = null)}
			onToggleFavorite={() => toggleFavorite(openDetail!.seed)}
		/>
	{/if}
</div>

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
	.bg-preview {
		display: flex;
		align-items: center;
		gap: 6px;
		margin-top: 4px;
		font-size: 12px;
		color: #ccc;
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

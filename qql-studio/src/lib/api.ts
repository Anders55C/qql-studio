import { invoke } from '@tauri-apps/api/core';

export type TraitChoices = {
	flowField: string | null;
	turbulence: string | null;
	margin: string | null;
	colorVariety: string | null;
	colorMode: string | null;
	structure: string | null;
	bullseye1: boolean | null;
	bullseye2: boolean | null;
	bullseye3: boolean | null;
	bullseye7: boolean | null;
	ringThickness: string | null;
	ringSize: string | null;
	sizeVariety: string | null;
	colorPalette: string | null;
	spacing: string | null;
};

export type TraitsWire = {
	flowField: string;
	turbulence: string;
	margin: string;
	colorVariety: string;
	colorMode: string;
	structure: string;
	bullseye1: boolean;
	bullseye2: boolean;
	bullseye3: boolean;
	bullseye7: boolean;
	ringThickness: string;
	ringSize: string;
	sizeVariety: string;
	colorPalette: string;
	spacing: string;
	version: string;
};

export type RingCountBucket = { rings: number; count: number };
export type RadiusBuckets = { small: number; medium: number; large: number };
export type Quadrants = {
	topLeft: number;
	topRight: number;
	bottomLeft: number;
	bottomRight: number;
};
export type FormationDims = {
	horizontal: number;
	vertical: number;
	total: number;
	stepFrac: number;
	skipOdds: number;
	actualChunks: number;
};
export type ShadowsInfo = {
	numCirclesTarget: number;
	actualCircles: number;
	pSquare: number;
	columnarSquare: boolean;
	outwardRadial: boolean;
};
export type OrbitalInfo = {
	ringBands: number;
	minSplits: number;
	maxSplits: number;
	splitsUsed: number[];
	centerOnCanvas: boolean;
	centerXFrac: number;
	centerYFrac: number;
	centerXCategory: number;
	centerYCategory: number;
	baseStepFrac: number;
	radialGroupStepFrac: number;
	splitOffsetDeg: number;
};
export type LayoutStats = {
	numPoints: number;
	colorsUsed: string[];
	ringCounts: RingCountBucket[];
	radiusBuckets: RadiusBuckets;
	quadrants: Quadrants;
	backgroundColor: string;
	formationDims: FormationDims | null;
	orbitalInfo: OrbitalInfo | null;
	shadowsInfo: ShadowsInfo | null;
	curveLength: number;
	splatterOdds: number;
};

export type ColorInfo = { name: string; swatch: string };
export type PaletteColors = {
	palette: string;
	backgrounds: ColorInfo[];
	primaries: ColorInfo[];
};

export type Candidate = {
	seed: string;
	traits: TraitsWire;
	stats: LayoutStats | null;
};

export type TraitMeta = { name: string; options: string[] };

export type CornerConcentration =
	| 'TL'
	| 'TR'
	| 'BL'
	| 'BR'
	| 'AnyLeft'
	| 'AnyRight';

export type CharacteristicFilter = {
	minPoints?: number;
	maxPoints?: number;
	minColors?: number;
	maxColors?: number;
	requiredColors?: string[];
	minSmall?: number;
	maxSmall?: number;
	minMedium?: number;
	maxMedium?: number;
	minLarge?: number;
	maxLarge?: number;
	minTl?: number;
	maxTl?: number;
	minTr?: number;
	maxTr?: number;
	minBl?: number;
	maxBl?: number;
	minBr?: number;
	maxBr?: number;
	cornerConcentration?: CornerConcentration;
	cornerRatio?: number;
	minHorizontalSections?: number;
	maxHorizontalSections?: number;
	minVerticalSections?: number;
	maxVerticalSections?: number;
	minTotalSections?: number;
	maxTotalSections?: number;
	allowedFormationStepBp?: number[];
	allowedFormationSkipBp?: number[];
	minFormationChunks?: number;
	maxFormationChunks?: number;
	minRingBands?: number;
	maxRingBands?: number;
	allowedOrbitalSplits?: number[];
	requiredOrbitalSplits?: number[];
	allowedOrbitalCenterXCategories?: number[];
	allowedOrbitalCenterYCategories?: number[];
	allowedOrbitalBaseStepBp?: number[];
	allowedOrbitalRadialStepBp?: number[];
	allowedCurveLengths?: number[];
	minSplitOffsetDeg?: number;
	maxSplitOffsetDeg?: number;
	splatterMode?: 'none' | 'any' | 'light' | 'moderate' | 'heavy';
	minSplatterOdds?: number;
	maxSplatterOdds?: number;
	allowedShadowsNumCircles?: number[];
	shadowsFillStyle?: 'all_radial' | 'mixed' | 'all_square';
	minShadowsActualCircles?: number;
	maxShadowsActualCircles?: number;
	shadowsColumnarSquare?: boolean;
	shadowsOutwardRadial?: boolean;
	backgroundColor?: string;
	ringColors?: string[];
	exactRingColors?: boolean;
};

export type GenerateMode = 'construct' | 'search';

export type GenerateRequest = {
	address: string;
	choices: TraitChoices;
	mode: GenerateMode;
	count: number;
	maxAttempts?: number;
	runLayout: boolean;
	characteristicFilter?: CharacteristicFilter;
	rngSeed?: number;
	workers?: number;
	minAngleDeg?: number;
	maxAngleDeg?: number;
	layoutTimeoutMs?: number;
};

export type RenderResult = { pngDataUrl: string; stats: LayoutStats };

export type GenerateProgress = {
	attempts: number;
	found: number;
	maxAttempts: number;
	want: number;
};

export type GenerateOutcome = {
	candidates: Candidate[];
	attempts: number;
	maxAttempts: number;
	cancelled: boolean;
};

export const listTraits = () => invoke<TraitMeta[]>('list_traits');
export const listPaletteColors = () =>
	invoke<PaletteColors[]>('list_palette_colors');
export const decodeSeed = (seedHex: string) =>
	invoke<TraitsWire>('decode_seed', { seedHex });
export const generateCandidates = (request: GenerateRequest) =>
	invoke<GenerateOutcome>('generate_candidates', { request });
export const cancelSearch = () => invoke<void>('cancel_search');
export const renderSeed = (seedHex: string, width: number) =>
	invoke<RenderResult>('render_seed', { seedHex, width });
export const saveSeedPng = (seed: string, width: number, folder: string) =>
	invoke<string>('save_seed_png', { seedHex: seed, width, folder });
export const deleteSeedPng = (folder: string, seed: string) =>
	invoke<void>('delete_seed_png', { folder, seedHex: seed });
export const exportSeedPng = (seed: string, width: number, path: string) =>
	invoke<string>('export_seed_png', { seedHex: seed, width, path });

export type AnimationProgress = { done: number; total: number; phase: string };
export const animationPreview = (seed: string, width: number, frames: number) =>
	invoke<string[]>('animation_preview', { seedHex: seed, width, frames });
export const exportAnimation = (
	seed: string,
	width: number,
	frames: number,
	format: 'apng' | 'frames',
	delayMs: number,
	holdMs: number,
	dest: string
) =>
	invoke<string>('export_animation', {
		seedHex: seed,
		width,
		frames,
		format,
		delayMs,
		holdMs,
		dest
	});
export const cancelAnimation = () => invoke<void>('cancel_animation');
export const layoutSummary = (seedHex: string) =>
	invoke<LayoutStats>('layout_summary', { seedHex });
export const randomSeedForAddress = (address: string) =>
	invoke<string>('random_seed_for_address', { address });

export const emptyChoices = (): TraitChoices => ({
	flowField: null,
	turbulence: null,
	margin: null,
	colorVariety: null,
	colorMode: null,
	structure: null,
	bullseye1: null,
	bullseye2: null,
	bullseye3: null,
	bullseye7: null,
	ringThickness: null,
	ringSize: null,
	sizeVariety: null,
	colorPalette: null,
	spacing: null
});

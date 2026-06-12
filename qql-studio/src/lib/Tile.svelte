<script lang="ts">
	import { type Candidate } from './api';
	import { queuedRenderSeed } from './renderQueue';

	type Props = {
		candidate: Candidate;
		thumbWidth?: number;
		favorited?: boolean;
		onOpen?: (c: Candidate) => void;
		onRendered?: (success: boolean) => void;
		onToggleFavorite?: () => Promise<void> | void;
	};
	let {
		candidate,
		thumbWidth = 480,
		favorited = false,
		onOpen,
		onRendered,
		onToggleFavorite
	}: Props = $props();

	let pngUrl = $state<string | null>(null);
	let loading = $state(false);
	let error = $state<string | null>(null);
	let reported = false;
	// Saving/deleting the favorite PNG re-renders the seed at full size, so the
	// heart shows a busy state while that's in flight.
	let savingFavorite = $state(false);

	async function toggleFavorite() {
		if (savingFavorite) return;
		savingFavorite = true;
		try {
			await onToggleFavorite?.();
		} finally {
			savingFavorite = false;
		}
	}

	async function load() {
		if (pngUrl || loading) return;
		loading = true;
		error = null;
		try {
			const r = await queuedRenderSeed(candidate.seed, thumbWidth);
			pngUrl = r.pngDataUrl;
			if (!reported) {
				reported = true;
				onRendered?.(true);
			}
		} catch (e) {
			error = String(e);
			if (!reported) {
				reported = true;
				onRendered?.(false);
			}
		} finally {
			loading = false;
		}
	}

	$effect(() => {
		// Auto-render on mount.
		load();
	});
</script>

<div class="tile">
	<div class="art-wrap">
		<button class="art" onclick={() => onOpen?.(candidate)} aria-label="Open detail">
			{#if pngUrl}
				<img src={pngUrl} alt="QQL render" />
			{:else if loading}
				<div class="placeholder">rendering…</div>
			{:else if error}
				<div class="placeholder err">{error}</div>
			{:else}
				<div class="placeholder">queued</div>
			{/if}
		</button>
		<button
			class="heart"
			class:on={favorited}
			class:busy={savingFavorite}
			disabled={savingFavorite}
			onclick={toggleFavorite}
			aria-label={favorited ? 'Remove favorite' : 'Save favorite'}
			title={favorited ? 'Saved — click to remove' : 'Save to favorites folder'}
		>
			{#if savingFavorite}
				<span class="spin">◌</span>
			{:else if favorited}
				♥
			{:else}
				♡
			{/if}
		</button>
	</div>
	<div class="meta">
		<div class="seed" title={candidate.seed}>
			{candidate.seed.slice(0, 10)}…{candidate.seed.slice(-6)}
		</div>
		<div class="traits">
			<span>{candidate.traits.colorPalette}</span>
			<span>·</span>
			<span>{candidate.traits.flowField}</span>
			<span>·</span>
			<span>{candidate.traits.structure}</span>
		</div>
		{#if candidate.stats}
			<div class="stats">
				{candidate.stats.numPoints.toLocaleString()} pts ·
				{candidate.stats.colorsUsed.length} colors
			</div>
			<div class="buckets">
				<span class="bk small" title="Small (≤1.2% canvas)">
					sm {candidate.stats.radiusBuckets.small.toLocaleString()}
				</span>
				<span class="bk medium" title="Medium (1.2–3% canvas)">
					md {candidate.stats.radiusBuckets.medium.toLocaleString()}
				</span>
				<span class="bk large" title="Large (>3% canvas)">
					lg {candidate.stats.radiusBuckets.large.toLocaleString()}
				</span>
			</div>
		{/if}
	</div>
</div>

<style>
	.tile {
		display: flex;
		flex-direction: column;
		gap: 6px;
		background: #1a1a1a;
		border: 1px solid #2a2a2a;
		border-radius: 8px;
		overflow: hidden;
	}
	.art-wrap {
		position: relative;
	}
	.art {
		all: unset;
		cursor: pointer;
		aspect-ratio: 4 / 5;
		display: block;
		background: #0d0d0d;
	}
	.heart {
		position: absolute;
		top: 8px;
		right: 8px;
		width: 30px;
		height: 30px;
		display: grid;
		place-items: center;
		padding: 0;
		border: none;
		border-radius: 50%;
		background: rgba(0, 0, 0, 0.45);
		color: #fff;
		font-size: 17px;
		line-height: 1;
		cursor: pointer;
		opacity: 0.75;
		transition: opacity 0.12s, background 0.12s, transform 0.08s;
	}
	.heart:hover {
		opacity: 1;
		background: rgba(0, 0, 0, 0.65);
	}
	.heart:active {
		transform: scale(0.9);
	}
	.heart.on {
		color: #ff4d6d;
		opacity: 1;
	}
	.heart.busy {
		cursor: default;
	}
	.heart .spin {
		display: inline-block;
		animation: heart-spin 0.8s linear infinite;
	}
	@keyframes heart-spin {
		to {
			transform: rotate(360deg);
		}
	}
	.art img {
		display: block;
		width: 100%;
		height: 100%;
		object-fit: contain;
	}
	.placeholder {
		display: grid;
		place-items: center;
		width: 100%;
		height: 100%;
		color: #888;
		font-size: 13px;
	}
	.placeholder.err {
		color: #d66;
		padding: 8px;
		text-align: center;
		font-family: ui-monospace, monospace;
		font-size: 11px;
	}
	.meta {
		padding: 6px 10px 10px;
		font-size: 11px;
		display: flex;
		flex-direction: column;
		gap: 2px;
		color: #ccc;
	}
	.seed {
		font-family: ui-monospace, SFMono-Regular, monospace;
		color: #aaa;
	}
	.traits {
		display: flex;
		gap: 4px;
		flex-wrap: wrap;
		color: #ddd;
	}
	.stats {
		color: #888;
	}
	.buckets {
		display: flex;
		gap: 4px;
		margin-top: 2px;
	}
	.bk {
		font-family: ui-monospace, monospace;
		font-size: 10px;
		padding: 1px 5px;
		border-radius: 99px;
		background: #232323;
	}
	.bk.small {
		color: #88a;
	}
	.bk.medium {
		color: #cc9;
	}
	.bk.large {
		color: #d96;
	}
</style>

<script lang="ts">
	import { exportSeedPng, renderSeed, type Candidate } from './api';
	import { save as saveDialog } from '@tauri-apps/plugin-dialog';

	type Props = {
		candidate: Candidate;
		favorited?: boolean;
		onClose: () => void;
		onToggleFavorite?: () => Promise<void> | void;
	};
	let { candidate, favorited = false, onClose, onToggleFavorite }: Props = $props();

	let pngUrl = $state<string | null>(null);
	let loading = $state(false);
	let error = $state<string | null>(null);
	let width = $state(2400);
	let savingFavorite = $state(false);
	let exporting = $state(false);

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
		loading = true;
		error = null;
		pngUrl = null;
		try {
			const r = await renderSeed(candidate.seed, width);
			pngUrl = r.pngDataUrl;
		} catch (e) {
			error = String(e);
		} finally {
			loading = false;
		}
	}

	function copy() {
		navigator.clipboard.writeText(candidate.seed);
	}

	// Deliberate one-off export: pick a destination, render at the Detail's
	// chosen width, and write it there. Independent of the favorites folder.
	async function saveAs() {
		if (exporting) return;
		const name = `${candidate.seed.replace(/^0x/, '')}.png`;
		const dest = await saveDialog({
			defaultPath: name,
			filters: [{ name: 'PNG image', extensions: ['png'] }]
		});
		if (!dest) return; // cancelled
		exporting = true;
		error = null;
		try {
			await exportSeedPng(candidate.seed, width, dest);
		} catch (e) {
			error = `Export failed: ${e}`;
		} finally {
			exporting = false;
		}
	}

	$effect(() => {
		load();
	});

	const traitRows = $derived.by(() => {
		const t = candidate.traits;
		return [
			['Flow field', t.flowField],
			['Turbulence', t.turbulence],
			['Margin', t.margin],
			['Color variety', t.colorVariety],
			['Color mode', t.colorMode],
			['Structure', t.structure],
			['Bullseye 1', t.bullseye1 ? 'yes' : 'no'],
			['Bullseye 2', t.bullseye2 ? 'yes (fallback)' : 'no'],
			['Bullseye 3', t.bullseye3 ? 'yes' : 'no'],
			['Bullseye 7', t.bullseye7 ? 'yes' : 'no'],
			['Ring thickness', t.ringThickness],
			['Ring size', t.ringSize],
			['Size variety', t.sizeVariety],
			['Color palette', t.colorPalette],
			['Spacing', t.spacing],
			['Version', t.version]
		] as [string, string][];
	});
</script>

<div
	class="backdrop"
	onclick={onClose}
	onkeydown={(e) => e.key === 'Escape' && onClose()}
	role="button"
	tabindex="-1"
></div>
<div class="panel" role="dialog" aria-modal="true">
	<div class="art-pane">
		{#if pngUrl}
			<img src={pngUrl} alt="QQL render" />
		{:else if loading}
			<div class="placeholder">rendering at {width}px…</div>
		{:else if error}
			<div class="placeholder err">{error}</div>
		{/if}
	</div>
	<div class="info-pane">
		<div class="header">
			<button class="close" onclick={onClose} aria-label="Close">×</button>
			<h2>Seed</h2>
			<div class="seed-row">
				<code>{candidate.seed}</code>
				<button onclick={copy}>copy</button>
			</div>
		</div>
		<div class="actions">
			<button
				class="fav"
				class:on={favorited}
				disabled={savingFavorite}
				onclick={toggleFavorite}
				title={favorited
					? 'Saved to favorites folder — click to remove'
					: 'Save to favorites folder'}
			>
				{#if savingFavorite}
					◌ working…
				{:else if favorited}
					♥ Saved
				{:else}
					♡ Save
				{/if}
			</button>
			<label>
				Render width:
				<select bind:value={width}>
					<option value={1200}>1200</option>
					<option value={2400}>2400</option>
					<option value={4800}>4800</option>
					<option value={9600}>9600</option>
				</select>
			</label>
			<button onclick={load} disabled={loading}>re-render</button>
			<button onclick={saveAs} disabled={exporting}>
				{exporting ? 'Saving…' : 'Save As…'}
			</button>
		</div>
		<h3>Traits</h3>
		<table class="traits">
			<tbody>
				{#each traitRows as [k, v]}
					<tr>
						<th>{k}</th>
						<td>{v}</td>
					</tr>
				{/each}
			</tbody>
		</table>
		{#if candidate.stats}
			<h3>Layout stats</h3>
			<div class="stats-line">
				<strong>{candidate.stats.numPoints.toLocaleString()}</strong> points,
				<strong>{candidate.stats.colorsUsed.length}</strong> colors
			</div>
			<div class="bg-line">
				Background: <strong>{candidate.stats.backgroundColor}</strong>
			</div>
			{#if candidate.stats.formationDims}
				<div class="bg-line">
					Sections:
					<strong>
						{candidate.stats.formationDims.horizontal} ×
						{candidate.stats.formationDims.vertical}
					</strong>
					({candidate.stats.formationDims.total} total)
				</div>
			{/if}
			{#if candidate.stats.shadowsInfo}
				{@const si = candidate.stats.shadowsInfo}
				{@const fillStyle =
					si.pSquare <= 0.1
						? 'all radial'
						: si.pSquare >= 0.9
							? 'all square'
							: 'mixed'}
				<div class="bg-line">
					Circles:
					<strong>{si.actualCircles} / {si.numCirclesTarget}</strong>
					· fill <strong>{fillStyle}</strong>
				</div>
				<div class="bg-line">
					Square: <strong>{si.columnarSquare ? 'col-major' : 'row-major'}</strong>
					· radial:
					<strong>
						{si.outwardRadial ? 'outside → in' : 'inside → out'}
					</strong>
				</div>
			{/if}
			{#if candidate.stats.orbitalInfo}
				{@const oi = candidate.stats.orbitalInfo}
				{@const fmt = (f: number) => {
					const pct = Math.round(f * 1000) / 1000;
					const map: Record<string, string> = {
						'0.333': '⅓',
						'0.5': '½',
						'0.667': '⅔',
						'-0.333': '-⅓',
						'1.333': '4/3',
						'-1.6': '-1.6',
						'1.6': '1.6'
					};
					return map[pct.toFixed(3)] ?? pct.toString();
				}}
				<div class="bg-line">
					Ring bands:
					<strong>{oi.ringBands}</strong>
					· splits used
					<strong>{oi.splitsUsed.join(', ') || 'none'}</strong>
				</div>
				<div class="bg-line">
					Center:
					<strong>({fmt(oi.centerXFrac)}, {fmt(oi.centerYFrac)})</strong>
					<span class="muted">
						· {oi.centerOnCanvas ? 'on canvas' : 'off canvas'}
					</span>
				</div>
				<div class="bg-line">
					Spacing
					<strong>{(oi.baseStepFrac * 100).toFixed(0)}%</strong>
					· band thickness
					<strong>{(oi.radialGroupStepFrac * 100).toFixed(0)}%</strong>
					· rotation
					<strong>{oi.splitOffsetDeg.toFixed(0)}°</strong>
				</div>
			{/if}
			<div class="bg-line">
				Curve length: <strong>{candidate.stats.curveLength}</strong>
			</div>
			<div class="bg-line">
				Splatter:
				<strong>
					{#if candidate.stats.splatterOdds <= 0}
						none
					{:else if candidate.stats.splatterOdds <= 0.005}
						light
					{:else if candidate.stats.splatterOdds < 0.08}
						moderate
					{:else if candidate.stats.splatterOdds < 0.5}
						heavy
					{:else}
						extreme
					{/if}
				</strong>
				<span class="muted">
					· odds {candidate.stats.splatterOdds.toFixed(3)}
				</span>
			</div>
			<div class="bucket-table">
				<div class="bucket-cell">
					<div class="bucket-name small">Small</div>
					<div class="bucket-value">
						{candidate.stats.radiusBuckets.small.toLocaleString()}
					</div>
					<div class="bucket-range">≤ 1.2%</div>
				</div>
				<div class="bucket-cell">
					<div class="bucket-name medium">Medium</div>
					<div class="bucket-value">
						{candidate.stats.radiusBuckets.medium.toLocaleString()}
					</div>
					<div class="bucket-range">1.2 – 3%</div>
				</div>
				<div class="bucket-cell">
					<div class="bucket-name large">Large</div>
					<div class="bucket-value">
						{candidate.stats.radiusBuckets.large.toLocaleString()}
					</div>
					<div class="bucket-range">&gt; 3%</div>
				</div>
			</div>
			{#if candidate.stats.quadrants}
				{@const q = candidate.stats.quadrants}
				{@const qTotal =
					q.topLeft + q.topRight + q.bottomLeft + q.bottomRight}
				{@const pct = (v: number) =>
					qTotal > 0 ? ((100 * v) / qTotal).toFixed(1) : '0.0'}
				<div class="quad-display">
					<div class="quad-display-row">
						<div class="quad-display-cell">
							<div class="quad-display-label">TL</div>
							<div class="quad-display-value">
								{q.topLeft.toLocaleString()}
							</div>
							<div class="quad-display-pct">{pct(q.topLeft)}%</div>
						</div>
						<div class="quad-display-cell">
							<div class="quad-display-label">TR</div>
							<div class="quad-display-value">
								{q.topRight.toLocaleString()}
							</div>
							<div class="quad-display-pct">{pct(q.topRight)}%</div>
						</div>
					</div>
					<div class="quad-display-row">
						<div class="quad-display-cell">
							<div class="quad-display-label">BL</div>
							<div class="quad-display-value">
								{q.bottomLeft.toLocaleString()}
							</div>
							<div class="quad-display-pct">{pct(q.bottomLeft)}%</div>
						</div>
						<div class="quad-display-cell">
							<div class="quad-display-label">BR</div>
							<div class="quad-display-value">
								{q.bottomRight.toLocaleString()}
							</div>
							<div class="quad-display-pct">{pct(q.bottomRight)}%</div>
						</div>
					</div>
				</div>
			{/if}
			<div class="color-chips">
				{#each candidate.stats.colorsUsed as c}
					<span class="chip">{c}</span>
				{/each}
			</div>
			<details>
				<summary>Ring counts</summary>
				<table class="ringcounts">
					<tbody>
						{#each candidate.stats.ringCounts as r}
							<tr>
								<th>{r.rings}-ring</th>
								<td>{r.count}</td>
							</tr>
						{/each}
					</tbody>
				</table>
			</details>
		{/if}
	</div>
</div>

<style>
	.backdrop {
		position: fixed;
		inset: 0;
		background: rgba(0, 0, 0, 0.7);
		z-index: 10;
	}
	.panel {
		position: fixed;
		inset: 24px;
		background: #1a1a1a;
		border: 1px solid #333;
		border-radius: 12px;
		z-index: 11;
		display: grid;
		grid-template-columns: 1fr 360px;
		overflow: hidden;
	}
	.art-pane {
		background: #0a0a0a;
		display: grid;
		place-items: center;
		overflow: auto;
		padding: 16px;
	}
	.art-pane img {
		max-width: 100%;
		max-height: 100%;
		object-fit: contain;
	}
	.placeholder {
		color: #888;
	}
	.placeholder.err {
		color: #d66;
		max-width: 60%;
		text-align: center;
		font-family: ui-monospace, monospace;
		font-size: 12px;
	}
	.info-pane {
		padding: 20px 20px 24px;
		overflow-y: auto;
		border-left: 1px solid #2a2a2a;
		font-size: 13px;
		color: #ccc;
	}
	.header {
		position: relative;
	}
	.close {
		position: absolute;
		top: -6px;
		right: -6px;
		background: transparent;
		border: none;
		color: #888;
		font-size: 24px;
		cursor: pointer;
	}
	.close:hover {
		color: #fff;
	}
	h2 {
		margin: 0 0 8px;
		font-size: 14px;
		text-transform: uppercase;
		color: #888;
		letter-spacing: 0.05em;
	}
	h3 {
		margin: 18px 0 6px;
		font-size: 13px;
		text-transform: uppercase;
		color: #888;
		letter-spacing: 0.05em;
	}
	.seed-row {
		display: flex;
		align-items: center;
		gap: 8px;
	}
	.seed-row code {
		font-size: 11px;
		word-break: break-all;
		flex: 1;
		font-family: ui-monospace, SFMono-Regular, monospace;
	}
	.actions {
		display: flex;
		gap: 8px;
		align-items: center;
		margin-top: 12px;
		flex-wrap: wrap;
	}
	.actions label {
		display: flex;
		align-items: center;
		gap: 6px;
	}
	button,
	select {
		background: #2a2a2a;
		color: #ddd;
		border: 1px solid #3a3a3a;
		border-radius: 4px;
		padding: 4px 10px;
		cursor: pointer;
		font-size: 12px;
	}
	button:hover:not(:disabled) {
		background: #3a3a3a;
	}
	button:disabled {
		opacity: 0.5;
		cursor: default;
	}
	button.fav.on {
		color: #ff4d6d;
		border-color: #5a2a33;
		background: #2a1a1e;
	}
	button.fav.on:hover:not(:disabled) {
		background: #3a2228;
	}
	table.traits,
	table.ringcounts {
		width: 100%;
		border-collapse: collapse;
		font-size: 12px;
	}
	table.traits th,
	table.traits td,
	table.ringcounts th,
	table.ringcounts td {
		padding: 3px 0;
		text-align: left;
		border-bottom: 1px solid #222;
	}
	table.traits th,
	table.ringcounts th {
		font-weight: 400;
		color: #888;
		width: 40%;
	}
	table.traits td,
	table.ringcounts td {
		color: #ddd;
	}
	.stats-line {
		color: #aaa;
	}
	.stats-line strong {
		color: #fff;
	}
	.bg-line {
		margin-top: 4px;
		font-size: 12px;
		color: #aaa;
	}
	.bg-line strong {
		color: #fff;
	}
	.bg-line .muted {
		color: #777;
		margin-left: 2px;
	}
	.bucket-table {
		display: grid;
		grid-template-columns: 1fr 1fr 1fr;
		gap: 6px;
		margin-top: 8px;
	}
	.bucket-cell {
		background: #0d0d0d;
		border: 1px solid #2a2a2a;
		border-radius: 4px;
		padding: 6px;
		text-align: center;
	}
	.bucket-name {
		font-size: 10px;
		text-transform: uppercase;
		letter-spacing: 0.05em;
		margin-bottom: 2px;
	}
	.bucket-name.small {
		color: #88a;
	}
	.bucket-name.medium {
		color: #cc9;
	}
	.bucket-name.large {
		color: #d96;
	}
	.bucket-value {
		font-size: 18px;
		font-weight: 500;
		color: #fff;
		font-variant-numeric: tabular-nums;
	}
	.bucket-range {
		font-size: 10px;
		color: #666;
		margin-top: 2px;
	}
	.quad-display {
		margin-top: 10px;
		display: flex;
		flex-direction: column;
		gap: 4px;
	}
	.quad-display-row {
		display: grid;
		grid-template-columns: 1fr 1fr;
		gap: 4px;
	}
	.quad-display-cell {
		background: #0d0d0d;
		border: 1px solid #2a2a2a;
		border-radius: 4px;
		padding: 6px 8px;
		text-align: center;
	}
	.quad-display-label {
		font-size: 10px;
		color: #888;
		font-family: ui-monospace, monospace;
	}
	.quad-display-value {
		font-size: 15px;
		font-weight: 500;
		color: #fff;
		font-variant-numeric: tabular-nums;
	}
	.quad-display-pct {
		font-size: 10px;
		color: #666;
	}
	.color-chips {
		display: flex;
		flex-wrap: wrap;
		gap: 4px;
		margin-top: 12px;
	}
	.chip {
		background: #2a2a2a;
		padding: 2px 8px;
		border-radius: 99px;
		font-size: 11px;
	}
	details summary {
		cursor: pointer;
		color: #888;
		font-size: 12px;
		margin-top: 8px;
	}
</style>

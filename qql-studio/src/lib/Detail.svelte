<script lang="ts">
	import {
		animationPreview,
		cancelAnimation,
		exportAnimation,
		exportSeedPng,
		renderSeed,
		type AnimationProgress,
		type Candidate,
		type LayoutStats
	} from './api';
	import { save as saveDialog, open as openDialog } from '@tauri-apps/plugin-dialog';
	import { listen, type UnlistenFn } from '@tauri-apps/api/event';
	import { onDestroy, onMount } from 'svelte';

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
	// Width used for Save As. The on-screen preview renders at screen size
	// instead (see previewWidth): a 9600px preview can use well over a GB.
	let width = $state(2400);
	let artPane = $state<HTMLDivElement | null>(null);
	// Device pixels needed to show the 4:5 image sharply in the art pane,
	// capped at 2400. Measured once the pane exists.
	const previewWidth = $derived.by(() => {
		if (!artPane) return null;
		const fitCss = Math.min(artPane.clientWidth, artPane.clientHeight * 0.8);
		const px = Math.ceil(fitCss * (window.devicePixelRatio || 1));
		return Math.min(2400, Math.max(800, Math.ceil(px / 100) * 100));
	});
	let saveNote = $state<{ text: string; err: boolean } | null>(null);
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

	// Inflate = qqlrs' inflate_draw_radius. Paint-time only, so it re-renders the
	// same layout with every point forced to a small minimum radius, revealing
	// dots that would otherwise be invisible.
	let inflate = $state(false);
	// Searches that only use structure filters skip the layout, so the
	// candidate may arrive without stats; the render below supplies them.
	let renderedStats = $state<LayoutStats | null>(null);
	const stats = $derived(candidate.stats ?? renderedStats);

	// Each render gets a ticket; only the latest one may update the view, so a
	// slow older render can't land on top of a newer one.
	let renderTicket = 0;
	async function load() {
		const w = previewWidth;
		if (w == null) return;
		const ticket = ++renderTicket;
		loading = true;
		error = null;
		pngUrl = null;
		try {
			const r = await renderSeed(candidate.seed, w, inflate);
			if (ticket !== renderTicket) return;
			pngUrl = r.pngDataUrl;
			renderedStats = r.stats;
		} catch (e) {
			if (ticket !== renderTicket) return;
			error = String(e);
		} finally {
			if (ticket === renderTicket) loading = false;
		}
	}

	function copy() {
		navigator.clipboard.writeText(candidate.seed);
	}

	// Deliberate one-off export: pick a destination, render at the Detail's
	// chosen width, and write it there. Independent of the favorites folder.
	async function saveAs() {
		if (exporting) return;
		const name = `${candidate.seed}${inflate ? '-inflated' : ''}.png`;
		const dest = await saveDialog({
			defaultPath: name,
			filters: [{ name: 'PNG image', extensions: ['png'] }]
		});
		if (!dest) return; // cancelled
		exporting = true;
		saveNote = null;
		try {
			await exportSeedPng(candidate.seed, width, dest, inflate);
			saveNote = { text: `Saved ${width}px PNG to ${dest}`, err: false };
		} catch (e) {
			saveNote = { text: `Save failed: ${e}`, err: true };
		} finally {
			exporting = false;
		}
	}

	// ---- Animation ----------------------------------------------------------
	let showAnim = $state(false);
	let animFrameCount = $state(60);
	let animSize = $state(720);
	let animFps = $state(20);
	let animHoldMs = $state(600);
	let frames = $state<string[] | null>(null);
	let frameIdx = $state(0);
	let playing = $state(false);
	let building = $state(false);
	let animProgress = $state<AnimationProgress | null>(null);
	let animError = $state<string | null>(null);
	let timer: ReturnType<typeof setInterval> | null = null;
	let unlistenProg: UnlistenFn | null = null;

	const delayMs = () => Math.max(16, Math.round(1000 / animFps));

	function stopPlayer() {
		if (timer) {
			clearInterval(timer);
			timer = null;
		}
		playing = false;
	}
	function startPlayer() {
		if (!frames || frames.length === 0) return;
		stopPlayer();
		playing = true;
		timer = setInterval(() => {
			if (!frames) return;
			frameIdx = (frameIdx + 1) % frames.length;
		}, delayMs());
	}

	// A user-initiated cancel surfaces as an error string from the backend;
	// don't show it as a failure.
	const isCancelled = (e: unknown) => /cancelled/i.test(String(e));

	async function buildAnimation() {
		// Also guard against `exporting`: build and export share one backend
		// cancel flag and one progress channel, so never run both at once.
		if (building || exporting) return;
		stopPlayer();
		building = true;
		animError = null;
		animProgress = null;
		frames = null;
		frameIdx = 0;
		try {
			frames = await animationPreview(
				candidate.seed,
				animSize,
				animFrameCount,
				stats?.numPoints
			);
			frameIdx = 0;
			startPlayer();
		} catch (e) {
			if (!isCancelled(e)) animError = String(e);
		} finally {
			building = false;
			animProgress = null;
		}
	}

	async function exportApng() {
		if (exporting || building) return;
		const dest = await saveDialog({
			defaultPath: `${candidate.seed}_anim.png`,
			filters: [{ name: 'Animated PNG', extensions: ['png'] }]
		});
		if (!dest) return;
		exporting = true;
		animError = null;
		animProgress = null;
		try {
			await exportAnimation(
				candidate.seed,
				animSize,
				animFrameCount,
				'apng',
				delayMs(),
				animHoldMs,
				dest,
				stats?.numPoints
			);
		} catch (e) {
			if (!isCancelled(e)) animError = `Export failed: ${e}`;
		} finally {
			exporting = false;
			animProgress = null;
		}
	}

	async function exportFrames() {
		if (exporting || building) return;
		const folder = await openDialog({
			directory: true,
			title: 'Choose a folder for the frame sequence'
		});
		if (typeof folder !== 'string') return;
		const dest = `${folder}/${candidate.seed}_frames`;
		exporting = true;
		animError = null;
		animProgress = null;
		try {
			await exportAnimation(
				candidate.seed,
				animSize,
				animFrameCount,
				'frames',
				delayMs(),
				animHoldMs,
				dest,
				stats?.numPoints
			);
		} catch (e) {
			if (!isCancelled(e)) animError = `Export failed: ${e}`;
		} finally {
			exporting = false;
			animProgress = null;
		}
	}

	onMount(async () => {
		unlistenProg = await listen<AnimationProgress>('animation-progress', (e) => {
			animProgress = e.payload;
		});
	});
	onDestroy(() => {
		// Closing the panel mid-render would otherwise leave the backend
		// burning CPU on frames nobody will consume.
		if (building || exporting) cancelAnimation();
		stopPlayer();
		unlistenProg?.();
	});

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

<svelte:window onkeydown={(e) => e.key === 'Escape' && onClose()} />
<div class="backdrop" onclick={onClose} role="presentation"></div>
<div class="panel" role="dialog" aria-modal="true">
	<div class="art-pane" bind:this={artPane}>
		{#if frames && showAnim}
			<img src={frames[frameIdx]} alt="QQL animation frame" />
		{:else if pngUrl}
			<img src={pngUrl} alt="QQL render" />
		{:else if loading}
			<div class="placeholder">rendering…</div>
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
			<label title="Size of the PNG that Save As writes">
				Export width:
				<select bind:value={width}>
					<option value={1200}>1200</option>
					<option value={2400}>2400</option>
					<option value={4800}>4800</option>
					<option value={9600}>9600</option>
				</select>
			</label>
			<label class="inflate-toggle" title="Give every point a small minimum radius so near-invisible dots get painted">
				<input type="checkbox" bind:checked={inflate} disabled={loading} />
				Inflate
			</label>
			<button onclick={load} disabled={loading}>re-render</button>
			<button onclick={saveAs} disabled={exporting}>
				{exporting ? 'Saving…' : 'Save As…'}
			</button>
			<button
				class:on={showAnim}
				onclick={() => {
					showAnim = !showAnim;
					// Collapsing hides the player controls, so also return the
					// art pane to the static render rather than a frozen frame.
					if (!showAnim) stopPlayer();
				}}
			>
				{showAnim ? '▾ Animate' : '▸ Animate'}
			</button>
		</div>
		{#if saveNote}
			<div class="save-note" class:err={saveNote.err}>{saveNote.text}</div>
		{/if}

		{#if showAnim}
			<div class="anim">
				<div class="anim-grid">
					<label>
						Frames
						<input type="number" min="2" max="600" step="1" bind:value={animFrameCount} />
					</label>
					<label>
						Size
						<select bind:value={animSize}>
							<option value={480}>480</option>
							<option value={720}>720</option>
							<option value={1080}>1080</option>
						</select>
					</label>
					<label>
						Speed (fps)
						<input
							type="number"
							min="2"
							max="60"
							step="1"
							bind:value={animFps}
							onchange={() => playing && startPlayer()}
						/>
					</label>
					<label>
						Hold end (ms)
						<input type="number" min="0" max="5000" step="100" bind:value={animHoldMs} />
					</label>
				</div>

				{#if building}
					<div class="anim-status">
						Rendering…
						{#if animProgress}{animProgress.done}/{animProgress.total}{/if}
						<button onclick={cancelAnimation}>cancel</button>
					</div>
				{:else}
					<button class="primary" onclick={buildAnimation} disabled={exporting}>
						{frames ? 'Re-build animation' : 'Build animation'}
					</button>
				{/if}

				{#if frames}
					<div class="player">
						<button onclick={() => (playing ? stopPlayer() : startPlayer())}>
							{playing ? '❚❚' : '▶'}
						</button>
						<input
							type="range"
							min="0"
							max={frames.length - 1}
							bind:value={frameIdx}
							oninput={stopPlayer}
						/>
						<span class="frame-num">{frameIdx + 1}/{frames.length}</span>
					</div>
					<div class="anim-export">
						<button onclick={exportApng} disabled={exporting || building}>
							{exporting ? 'Exporting…' : 'Export APNG'}
						</button>
						<button onclick={exportFrames} disabled={exporting || building}>Export frames…</button>
					</div>
					{#if exporting && animProgress}
						<div class="anim-status">
							{animProgress.phase}… {animProgress.done}/{animProgress.total}
						</div>
					{/if}
				{/if}

				{#if animError}<div class="anim-err">{animError}</div>{/if}
			</div>
		{/if}

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
		{#if stats}
			{@const st = stats}
			{@const so = st.splatterOdds}
			{@const splatterMatches = [
				so <= 0 && 'None',
				so > 0 && 'Has splatter',
				so > 0 && so <= 0.005 && 'Light only',
				so >= 0.01 && 'Moderate+',
				so >= 0.08 && 'Heavy+'
			].filter(Boolean)}
			{@const fl = st.flow}
			<h3>Layout stats <small class="muted">· named to match the filters</small></h3>

			<div class="stat-sec">
				<h4>Characteristics</h4>
				<div class="row"><span>Points</span><b>{st.numPoints.toLocaleString()}</b></div>
				<div class="row"><span>Colors</span><b>{st.colorsUsed.length}</b></div>
			</div>

			<div class="stat-sec">
				<h4>Radius buckets</h4>
				<div class="bucket-table">
					<div class="bucket-cell">
						<div class="bucket-name small">Small</div>
						<div class="bucket-value">{st.radiusBuckets.small.toLocaleString()}</div>
						<div class="bucket-range">≤ 1.2%</div>
					</div>
					<div class="bucket-cell">
						<div class="bucket-name medium">Medium</div>
						<div class="bucket-value">{st.radiusBuckets.medium.toLocaleString()}</div>
						<div class="bucket-range">1.2 – 3%</div>
					</div>
					<div class="bucket-cell">
						<div class="bucket-name large">Large</div>
						<div class="bucket-value">{st.radiusBuckets.large.toLocaleString()}</div>
						<div class="bucket-range">&gt; 3%</div>
					</div>
				</div>
			</div>

			<div class="stat-sec">
				<h4>Colors</h4>
				<div class="row"><span>Background</span><b>{st.backgroundColor}</b></div>
				<div class="row top">
					<span>Ring colors</span>
					<div class="color-chips">
						{#each st.colorsUsed as c}<span class="chip">{c}</span>{/each}
					</div>
				</div>
				<div class="row">
					<span>Splatter</span>
					<b>{splatterMatches.join(' · ')}</b>
				</div>
				<div class="row"><span>Odds range</span><b>{so.toFixed(3)}</b></div>
			</div>

			<div class="stat-sec">
				<h4>Composition</h4>
				<div class="row">
					<span>Flow angle</span>
					{#if fl?.linear && fl.angleDeg != null}
						<b>{fl.angleDeg.toFixed(1)}°</b>
					{:else}
						<b class="na">n/a — radial flow</b>
					{/if}
				</div>
				{#if fl && !fl.linear}
					<div class="row sub">
						<span>Flow</span>
						<span>
							{fl.inward ? 'inward' : 'outward'}, {fl.ccw ? 'counter-clockwise' : 'clockwise'}
							· circularity {fl.circularity?.toFixed(2)}
						</span>
					</div>
					{#if fl.centerXFrac != null && fl.centerYFrac != null}
						{@const onCanvas =
							fl.centerXFrac >= 0 && fl.centerXFrac <= 1 && fl.centerYFrac >= 0 && fl.centerYFrac <= 1}
						<div class="row sub">
							<span>Flow center</span>
							<span>
								{Math.round(fl.centerXFrac * 100)}% across, {Math.round(
									fl.centerYFrac * 100
								)}% down · {onCanvas ? 'on canvas' : 'off canvas'}
							</span>
						</div>
					{/if}
				{/if}
				{#if st.quadrants}
					{@const q = st.quadrants}
					{@const corners = [
						['Top-left', q.topLeft],
						['Top-right', q.topRight],
						['Bottom-left', q.bottomLeft],
						['Bottom-right', q.bottomRight]
					] as [string, number][]}
					{@const densest = corners.reduce((a, b) => (b[1] >= a[1] ? b : a))}
					{@const sparsest = corners.reduce((a, b) => (b[1] < a[1] ? b : a))}
					{@const qTotal = q.topLeft + q.topRight + q.bottomLeft + q.bottomRight}
					{@const pct = (v: number) => (qTotal > 0 ? ((100 * v) / qTotal).toFixed(1) : '0.0')}
					<div class="row">
						<span>Corner concentration</span>
						<b>{densest[0]} densest</b>
					</div>
					<div class="row">
						<span>Min ratio</span>
						<b>{sparsest[1] > 0 ? (densest[1] / sparsest[1]).toFixed(2) + '×' : '∞'}</b>
					</div>
					<div class="quad-display">
						<div class="quad-display-row">
							<div class="quad-display-cell">
								<div class="quad-display-label">TL</div>
								<div class="quad-display-value">{q.topLeft.toLocaleString()}</div>
								<div class="quad-display-pct">{pct(q.topLeft)}%</div>
							</div>
							<div class="quad-display-cell">
								<div class="quad-display-label">TR</div>
								<div class="quad-display-value">{q.topRight.toLocaleString()}</div>
								<div class="quad-display-pct">{pct(q.topRight)}%</div>
							</div>
						</div>
						<div class="quad-display-row">
							<div class="quad-display-cell">
								<div class="quad-display-label">BL</div>
								<div class="quad-display-value">{q.bottomLeft.toLocaleString()}</div>
								<div class="quad-display-pct">{pct(q.bottomLeft)}%</div>
							</div>
							<div class="quad-display-cell">
								<div class="quad-display-label">BR</div>
								<div class="quad-display-value">{q.bottomRight.toLocaleString()}</div>
								<div class="quad-display-pct">{pct(q.bottomRight)}%</div>
							</div>
						</div>
					</div>
				{/if}
				<div class="row"><span>Curve length</span><b>{st.curveLength}</b></div>
			</div>

			{#if st.formationDims}
				{@const fd = st.formationDims}
				<div class="stat-sec">
					<h4>Formation structure</h4>
					<div class="row"><span>Horizontal <small>columns</small></span><b>{fd.horizontal}</b></div>
					<div class="row"><span>Vertical <small>rows</small></span><b>{fd.vertical}</b></div>
					<div class="row"><span>Point spacing <small>% canvas</small></span><b>{+(fd.stepFrac * 100).toFixed(2)}</b></div>
					<div class="row"><span>Skip odds</span><b>{fd.skipOdds === 0 ? '0' : String(fd.skipOdds).replace(/^0/, '')}</b></div>
					<div class="row"><span>Actual chunks</span><b>{fd.actualChunks}</b></div>
				</div>
			{/if}

			{#if st.orbitalInfo}
				{@const oi = st.orbitalInfo}
				{@const posX = (c: number) => ['far ←', 'out ←', '⅓', '½', '⅔', 'out →', 'far →'][c + 3] ?? '?'}
				{@const posY = (c: number) => ['far ↑', 'out ↑', '⅓', '½', '⅔', 'out ↓', 'far ↓'][c + 3] ?? '?'}
				<div class="stat-sec">
					<h4>Orbital structure</h4>
					<div class="row"><span>Ring bands</span><b>{oi.ringBands}</b></div>
					<div class="row">
						<span>Splits used <small>Allowed / Required</small></span>
						<b>{oi.splitsUsed.join(', ') || 'none'}</b>
					</div>
					<div class="row">
						<span>Center X</span>
						<b>{posX(oi.centerXPosition)} <small class="muted">{oi.centerXFrac.toFixed(3)}</small></b>
					</div>
					<div class="row">
						<span>Center Y</span>
						<b>{posY(oi.centerYPosition)} <small class="muted">{oi.centerYFrac.toFixed(3)}</small></b>
					</div>
					<div class="row"><span>Point spacing <small>% canvas</small></span><b>{Math.round(oi.baseStepFrac * 100)}</b></div>
					<div class="row"><span>Band thickness <small>% canvas</small></span><b>{Math.round(oi.radialGroupStepFrac * 100)}</b></div>
					<div class="row"><span>Split offset</span><b>{oi.splitOffsetDeg.toFixed(0)}°</b></div>
				</div>
			{/if}

			{#if st.shadowsInfo}
				{@const si = st.shadowsInfo}
				<div class="stat-sec">
					<h4>Shadows structure</h4>
					<div class="row"><span>Target circles</span><b>{si.numCirclesTarget}</b></div>
					<div class="row">
						<span>Fill style</span>
						<b>{si.pSquare <= 0.1 ? 'All radial' : si.pSquare >= 0.9 ? 'All square' : 'Mixed'}</b>
					</div>
					<div class="row"><span>Actual circles</span><b>{si.actualCircles}</b></div>
					<div class="row"><span>Square direction</span><b>{si.columnarSquare ? 'Column-major' : 'Row-major'}</b></div>
					<div class="row"><span>Radial direction</span><b>{si.outwardRadial ? 'Outside → in' : 'Inside → out'}</b></div>
				</div>
			{/if}

			<details>
				<summary>Ring counts</summary>
				<table class="ringcounts">
					<tbody>
						{#each st.ringCounts as r}
							<tr><th>{r.rings}-ring</th><td>{r.count}</td></tr>
						{/each}
					</tbody>
				</table>
			</details>
		{/if}
	</div>
</div>

<style>
	.muted { color: #777; font-weight: 400; }
	.stat-sec { margin: 14px 0 4px; padding-top: 10px; border-top: 1px solid #222; }
	.stat-sec h4 { margin: 0 0 6px; font-size: 11px; text-transform: uppercase; letter-spacing: 0.08em; color: #888; font-weight: 600; }
	.stat-sec .row { display: flex; justify-content: space-between; align-items: baseline; gap: 12px; font-size: 12.5px; padding: 2px 0; }
	.stat-sec .row.top { align-items: flex-start; }
	.stat-sec .row > span:first-child { color: #9a9a9a; flex-shrink: 0; }
	.stat-sec .row small { color: #666; font-size: 10.5px; margin-left: 3px; }
	.stat-sec .row b { font-weight: 600; color: #e6e6e6; text-align: right; }
	.stat-sec .row b.na { color: #777; font-weight: 400; }
	.stat-sec .row.sub { font-size: 11.5px; color: #8a8a8a; padding-left: 10px; }
	.stat-sec .row.sub > span:last-child { text-align: right; }
	.stat-sec .color-chips { justify-content: flex-end; }
	.inflate-toggle {
		display: inline-flex;
		align-items: center;
		gap: 5px;
		font-size: 12px;
		color: #bbb;
		cursor: pointer;
		white-space: nowrap;
	}
	.inflate-toggle input {
		margin: 0;
		cursor: pointer;
	}
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
	.save-note {
		font-size: 11px;
		color: #8fc4a8;
		margin: -4px 0 8px;
		word-break: break-all;
	}
	.save-note.err {
		color: #d66;
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
	.anim {
		margin-top: 12px;
		padding: 12px;
		background: #0d0d0d;
		border: 1px solid #2a2a2a;
		border-radius: 6px;
		display: flex;
		flex-direction: column;
		gap: 10px;
	}
	.anim-grid {
		display: grid;
		grid-template-columns: 1fr 1fr;
		gap: 8px;
	}
	.anim-grid label {
		display: flex;
		flex-direction: column;
		gap: 3px;
		font-size: 11px;
		color: #888;
	}
	.anim-status {
		display: flex;
		align-items: center;
		gap: 8px;
		font-size: 12px;
		color: #aaa;
	}
	.player {
		display: flex;
		align-items: center;
		gap: 8px;
	}
	.player input[type='range'] {
		flex: 1;
	}
	.frame-num {
		font-family: ui-monospace, monospace;
		font-size: 11px;
		color: #888;
		min-width: 52px;
		text-align: right;
	}
	.anim-export {
		display: flex;
		gap: 8px;
	}
	.primary {
		background: #2f6f4f;
		border-color: #3a8a63;
		color: #eafff2;
	}
	.anim-err {
		color: #d66;
		font-size: 11px;
		font-family: ui-monospace, monospace;
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

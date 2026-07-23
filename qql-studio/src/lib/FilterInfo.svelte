<script lang="ts">
	/**
	 * Small "i" button that opens a popup explaining what a filter does.
	 * Drop it next to any filter heading:
	 *
	 *   <FilterInfo title="Flow angle">
	 *     <p>…explanation…</p>
	 *   </FilterInfo>
	 *
	 * The popup is a fixed-position card rather than an inline popover so it
	 * can't be clipped by the sidebar's own scroll container.
	 */
	import type { Snippet } from 'svelte';

	type Props = { title: string; children: Snippet };
	let { title, children }: Props = $props();

	let open = $state(false);

	function close() {
		open = false;
	}
</script>

<button
	type="button"
	class="info-btn"
	aria-label="About {title}"
	title="About {title}"
	onclick={(e) => {
		// The button often sits inside a <label>; stop the click from also
		// focusing/toggling the control that label points at.
		e.preventDefault();
		e.stopPropagation();
		open = !open;
	}}
>
	i
</button>

{#if open}
	<div
		class="info-backdrop"
		onclick={close}
		onkeydown={(e) => e.key === 'Escape' && close()}
		role="button"
		tabindex="-1"
	></div>
	<div class="info-card" role="dialog" aria-modal="true" aria-label={title}>
		<div class="info-head">
			<h3>{title}</h3>
			<button type="button" class="info-close" aria-label="Close" onclick={close}>×</button>
		</div>
		<div class="info-body">
			{@render children()}
		</div>
	</div>
{/if}

<svelte:window onkeydown={(e) => open && e.key === 'Escape' && close()} />

<style>
	/* These often sit inside an uppercase heading (.subhead / .label), which
	   would otherwise inherit into the button glyph and the whole popup. */
	.info-btn,
	.info-card {
		text-transform: none;
		letter-spacing: normal;
	}
	.info-btn {
		display: inline-flex;
		align-items: center;
		justify-content: center;
		width: 14px;
		height: 14px;
		margin-left: 5px;
		padding: 0;
		border: 1px solid #4a4a4a;
		border-radius: 50%;
		background: transparent;
		color: #8a8a8a;
		font-family: Georgia, 'Times New Roman', serif;
		font-style: italic;
		font-size: 10px;
		line-height: 1;
		cursor: pointer;
		vertical-align: middle;
		flex-shrink: 0;
	}
	.info-btn:hover {
		border-color: #7a7a7a;
		color: #ddd;
		background: #232323;
	}
	.info-backdrop {
		position: fixed;
		inset: 0;
		background: rgba(0, 0, 0, 0.6);
		z-index: 40;
	}
	.info-card {
		position: fixed;
		top: 50%;
		left: 50%;
		transform: translate(-50%, -50%);
		width: min(460px, calc(100vw - 48px));
		max-height: min(70vh, 620px);
		display: flex;
		flex-direction: column;
		background: #171717;
		border: 1px solid #333;
		border-radius: 10px;
		box-shadow: 0 18px 50px rgba(0, 0, 0, 0.6);
		z-index: 41;
	}
	.info-head {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: 10px;
		padding: 13px 15px;
		border-bottom: 1px solid #2a2a2a;
	}
	.info-head h3 {
		margin: 0;
		font-size: 13px;
		letter-spacing: 0.02em;
		color: #eee;
	}
	.info-close {
		border: 0;
		background: transparent;
		color: #888;
		font-size: 19px;
		line-height: 1;
		padding: 0 2px;
		cursor: pointer;
	}
	.info-close:hover {
		color: #ddd;
	}
	.info-body {
		padding: 13px 15px 16px;
		overflow-y: auto;
		font-size: 12.5px;
		line-height: 1.55;
		color: #c4c4c4;
	}
	/* Content styling lives here so each call site only writes plain markup. */
	.info-body :global(p) {
		margin: 0 0 10px;
	}
	.info-body :global(p:last-child) {
		margin-bottom: 0;
	}
	.info-body :global(strong) {
		color: #e8e8e8;
	}
	.info-body :global(em) {
		color: #d8d8d8;
		font-style: italic;
	}
	.info-body :global(ul) {
		margin: 0 0 10px;
		padding-left: 17px;
	}
	.info-body :global(li) {
		margin-bottom: 4px;
	}
	.info-body :global(.tip) {
		margin-top: 12px;
		padding: 9px 11px;
		background: rgba(181, 154, 106, 0.11);
		border-left: 2px solid rgba(181, 154, 106, 0.55);
		border-radius: 3px;
		color: #b59a6a;
	}
	.info-body :global(.tip strong) {
		color: #d4b87e;
	}
</style>

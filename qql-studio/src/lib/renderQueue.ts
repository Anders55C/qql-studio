// A small in-process concurrency limiter for renderSeed calls.
//
// Without this, every Tile fires its render request the moment it mounts and
// the Tauri backend services them in essentially nondeterministic order — so
// tiles in the grid appear in scattershot order rather than top-to-bottom.
//
// With this queue, at most `maxConcurrent` renders are in flight at once.
// Tile components mount in array order, so they enqueue in order, so they
// resolve in roughly the same order — you see the grid fill in waves of N
// instead of a random pop-corn pattern.

import { renderSeed, renderThumbnail, type RenderResult } from './api';

let maxConcurrent = 8;
let active = 0;
type Waiter = { resolve: () => void; reject: (e: Error) => void };
let waiters: Waiter[] = [];

/** Thrown to renders that were still queued when a new search started. */
export class StaleRenderError extends Error {
	constructor() {
		super('render dropped: a newer search started');
	}
}

/**
 * Drop every render still waiting in the queue. Called when a new search
 * starts, so thumbnails from the previous results (no longer on screen) don't
 * run first and compete with the new search. Renders already in flight finish.
 */
export function dropQueuedRenders() {
	const dropped = waiters;
	waiters = [];
	for (const w of dropped) w.reject(new StaleRenderError());
}

export function setRenderConcurrency(n: number) {
	const clamped = Math.max(1, Math.min(64, Math.floor(n)));
	if (clamped === maxConcurrent) return;
	maxConcurrent = clamped;
	pump();
}

function pump() {
	while (active < maxConcurrent && waiters.length > 0) {
		active += 1;
		const next = waiters.shift()!;
		next.resolve();
	}
}

function acquire(): Promise<void> {
	return new Promise<void>((resolve, reject) => {
		waiters.push({ resolve, reject });
		pump();
	});
}

function release() {
	active = Math.max(0, active - 1);
	pump();
}

/** Queued thumbnail render into the disk cache; resolves to the file path. */
export async function queuedRenderThumbnail(seedHex: string, width: number): Promise<string> {
	await acquire();
	try {
		return await renderThumbnail(seedHex, width);
	} finally {
		release();
	}
}

export async function queuedRenderSeed(
	seedHex: string,
	width: number
): Promise<RenderResult> {
	await acquire();
	try {
		return await renderSeed(seedHex, width);
	} finally {
		release();
	}
}

// A small in-process concurrency limiter for thumbnail renders.
//
// Without this, every Tile fires its render request the moment it mounts and
// the Tauri backend services them in essentially nondeterministic order — so
// tiles in the grid appear in scattershot order rather than top-to-bottom.
//
// With this queue, at most `maxConcurrent` renders are in flight at once.
// Waiting tiles near the screen go first (each tile reports that through its
// `near` callback); otherwise tiles start in the order they asked, which is
// grid order, so the grid fills top-to-bottom in waves of N.
//
// Interactive work (the inspector's render, saving a favorite, exports) runs
// outside the queue; while any is running, new thumbnails start at most one
// at a time so that work gets the CPU.

import { renderThumbnail } from './api';

let maxConcurrent = 8;
let active = 0;
let interactive = 0;
type Waiter = { resolve: () => void; reject: (e: Error) => void; near: () => boolean };
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
	const limit = interactive > 0 ? 1 : maxConcurrent;
	while (active < limit && waiters.length > 0) {
		// First waiting tile that's near the screen, else the oldest.
		let i = waiters.findIndex((w) => w.near());
		if (i < 0) i = 0;
		const [next] = waiters.splice(i, 1);
		active += 1;
		next.resolve();
	}
}

/** Re-check priorities, e.g. after the user scrolls new tiles into view. */
export function nudgeRenderQueue() {
	pump();
}

function acquire(near: () => boolean): Promise<void> {
	return new Promise<void>((resolve, reject) => {
		waiters.push({ resolve, reject, near });
		pump();
	});
}

function release() {
	active = Math.max(0, active - 1);
	pump();
}

/**
 * Queued thumbnail render into the disk cache; resolves to the file path.
 * `near` says whether the tile is currently near the screen.
 */
export async function queuedRenderThumbnail(
	seedHex: string,
	width: number,
	near: () => boolean = () => false
): Promise<string> {
	await acquire(near);
	try {
		return await renderThumbnail(seedHex, width);
	} finally {
		release();
	}
}

/**
 * Run interactive work (inspector render, favorite save, export) ahead of the
 * thumbnail queue: while it runs, new thumbnails start one at a time.
 */
export async function withPriority<T>(work: () => Promise<T>): Promise<T> {
	interactive += 1;
	try {
		return await work();
	} finally {
		interactive -= 1;
		pump();
	}
}

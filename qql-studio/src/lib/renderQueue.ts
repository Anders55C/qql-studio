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

import { renderSeed, type RenderResult } from './api';

let maxConcurrent = 8;
let active = 0;
const waiters: Array<() => void> = [];

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
		next();
	}
}

function acquire(): Promise<void> {
	return new Promise<void>((resolve) => {
		waiters.push(resolve);
		pump();
	});
}

function release() {
	active = Math.max(0, active - 1);
	pump();
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

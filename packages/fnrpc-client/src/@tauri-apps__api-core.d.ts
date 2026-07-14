declare module "@tauri-apps/api/core" {
	export function invoke<T = unknown>(
		cmd: string,
		args?: Record<string, unknown>,
	): Promise<T>;
	export function isTauri(): boolean;
	export class Channel<T = unknown> {
		constructor();
		onmessage: (msg: T) => void;
	}
}

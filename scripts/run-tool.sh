#!/usr/bin/env bash
set -euo pipefail

if [ "$#" -lt 1 ]; then
	echo "Usage: $0 <tool> [args...]" >&2
	exit 2
fi

tool="$1"
shift

resolve_tool_path() {
	local name="$1"

	if command -v "$name" >/dev/null 2>&1; then
		command -v "$name"
		return 0
	fi

	local -a candidates=()

	if [ -n "${CARGO_HOME:-}" ]; then
		candidates+=("${CARGO_HOME}/bin/${name}" "${CARGO_HOME}/bin/${name}.exe")
	fi

	if [ -n "${HOME:-}" ]; then
		candidates+=("${HOME}/.cargo/bin/${name}" "${HOME}/.cargo/bin/${name}.exe")
	fi

	if [ -n "${USERPROFILE:-}" ] && command -v cygpath >/dev/null 2>&1; then
		local userprofile_unix
		userprofile_unix="$(cygpath -u "${USERPROFILE}")"
		candidates+=("${userprofile_unix}/.cargo/bin/${name}" "${userprofile_unix}/.cargo/bin/${name}.exe")
	fi

	if [ -n "${USERNAME:-}" ]; then
		candidates+=("/c/Users/${USERNAME}/.cargo/bin/${name}" "/c/Users/${USERNAME}/.cargo/bin/${name}.exe")
		candidates+=("/mnt/c/Users/${USERNAME}/.cargo/bin/${name}" "/mnt/c/Users/${USERNAME}/.cargo/bin/${name}.exe")
	fi

	if [ -d "/mnt/c/Users" ]; then
		local user_dir
		for user_dir in /mnt/c/Users/*; do
			if [ -d "$user_dir/.cargo/bin" ]; then
				candidates+=("$user_dir/.cargo/bin/${name}" "$user_dir/.cargo/bin/${name}.exe")
			fi
		done
	fi

	if [ -d "/c/Users" ]; then
		local user_dir
		for user_dir in /c/Users/*; do
			if [ -d "$user_dir/.cargo/bin" ]; then
				candidates+=("$user_dir/.cargo/bin/${name}" "$user_dir/.cargo/bin/${name}.exe")
			fi
		done
	fi

	local candidate
	for candidate in "${candidates[@]}"; do
		if [ -x "$candidate" ]; then
			printf '%s\n' "$candidate"
			return 0
		fi
	done

	return 1
}

if ! resolved="$(resolve_tool_path "$tool")"; then
	echo "Error: '$tool' was not found in PATH or common Rust install locations." >&2
	echo "Install Rust via rustup and ensure ~/.cargo/bin is available to your shell." >&2
	exit 127
fi

exec "$resolved" "$@"

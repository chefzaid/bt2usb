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

	# Never execute a tool from another user's profile as a PATH fallback.

	local candidate
	for candidate in "${candidates[@]}"; do
		if [ -x "$candidate" ]; then
			printf '%s\n' "$candidate"
			return 0
		fi
	done

	# WSL may have a different Linux username and no USERPROFILE/USERNAME.
	# Ask Windows for the current user's profile instead of scanning every user.
	if command -v wslpath >/dev/null 2>&1 && command -v cmd.exe >/dev/null 2>&1; then
		local windows_profile windows_profile_unix
		if windows_profile="$(cmd.exe /d /c 'echo %USERPROFILE%' 2>/dev/null | tr -d '\r')" &&
			windows_profile_unix="$(wslpath -u "$windows_profile" 2>/dev/null)"; then
			for candidate in "$windows_profile_unix/.cargo/bin/$name" "$windows_profile_unix/.cargo/bin/$name.exe"; do
				if [ -x "$candidate" ]; then
					printf '%s\n' "$candidate"
					return 0
				fi
			done
		fi
	fi

	return 1
}

if ! resolved="$(resolve_tool_path "$tool")"; then
	echo "Error: '$tool' was not found in PATH or the current user's Rust install locations." >&2
	echo "Install Rust via rustup and ensure ~/.cargo/bin is available to your shell." >&2
	exit 127
fi

exec "$resolved" "$@"

#!/usr/bin/env bash
# Releases Orbit from main: writes the change files into CHANGELOG.md, sets the version and commits
# `chore(release): <version>`. The push of that commit runs the release (`.github/workflows/image.yml`). See RELEASE.md.
set -euo pipefail

OPTION="$*"
CHANGE_DIRECTORY=.changes
WORK_DIR="$(mktemp -d)"

BOLD=$'\033[1m' BLUE=$'\033[34m' GREEN=$'\033[32m' YELLOW=$'\033[33m' RED=$'\033[31m' DIM=$'\033[2m' RESET=$'\033[0m'

heading() { printf '\n  %s%s%s\n' "$BOLD" "$*" "$RESET"; }
info() { printf '  %sℹ%s %s\n' "$BLUE" "$RESET" "$*"; }
ok() { printf '  %s✔%s %s\n' "$GREEN" "$RESET" "$*"; }
warn() { printf '  %s⚠%s %s\n' "$YELLOW" "$RESET" "$*"; }
die() {
	printf '  %s✖%s %s\n' "$RED" "$RESET" "$*" >&2
	exit 1
}
confirm() {
	local answer
	printf '\n  %s [y/N] ' "$*"
	read -r answer
	[[ "$answer" =~ ^[Yy]$ ]]
}
hint() { printf '  %-26s%s%s%s\n' "$1" "$DIM" "$2" "$RESET"; }

cd "$(dirname "$0")/.."
trap 'rm -rf "$WORK_DIR"; tput cnorm 2> /dev/null || true' EXIT

main() {
	[ -z "$OPTION" ] || die "'$OPTION' is not an option, expected none"
	check_repository
	read_version
	read_changes
	select_version
	heading "Releasing v$new_version"
	update_changelog
	rm "$CHANGE_DIRECTORY"/*.md
	ok "$CHANGE_DIRECTORY is empty, CHANGELOG.md lists the changes now"
	bash scripts/version.sh "$new_version" > /dev/null
	ok "Cargo.toml, Cargo.lock, apps/web/package.json and bun.lock set $version → $BOLD$new_version$RESET"
	commit
	push
}

check_repository() {
	local counts behind ahead entries
	heading "Checking the repository"
	branch="$(git symbolic-ref --quiet --short HEAD)" || die "HEAD is detached, expected main"
	[ "$branch" = main ] || die "$branch releases nothing, only main does"
	[ -z "$(git status --porcelain)" ] || die "the working tree is not clean, commit or stash first"
	git fetch --quiet --prune --tags origin || die "could not fetch origin"
	counts="$(git rev-list --left-right --count "origin/main...HEAD")"
	read -r behind ahead <<< "$counts"
	[ "$behind$ahead" = 00 ] || die "main is $ahead commit(s) ahead of and $behind behind origin/main, expected it in sync"
	ok "main is clean and in sync with origin/main"
	entries="$(git ls-files "$CHANGE_DIRECTORY" | grep -c '\.md$' || true)"
	[ "$entries" != 0 ] || die "$CHANGE_DIRECTORY has no change files, describe the changes first with scripts/change.sh"
	ok "$CHANGE_DIRECTORY lists $BOLD$entries$RESET change(s), the notes of this release"
}

read_version() {
	version="$(bash scripts/version.sh)"
	git rev-parse --quiet --verify "refs/tags/v$version" > /dev/null \
		|| die "Cargo.toml sets $version, which has no tag v$version yet: release it first (run the Container image workflow on main)"
	ok "Orbit $BOLD$version$RESET is the latest release"
}

read_changes() {
	local file
	required=patch
	if grep -qx 'breaking: true' "$CHANGE_DIRECTORY"/*.md; then
		required=major
		# before 1.0, a minor release may break
		[ "${version%%.*}" != 0 ] || required=minor
	elif grep -qxE 'type: (added|changed|deprecated|removed)' "$CHANGE_DIRECTORY"/*.md; then
		required=minor
	fi
	info "the change files need a $BOLD$required$RESET release"
	for file in $(grep -lx 'breaking: true' "$CHANGE_DIRECTORY"/*.md || true); do
		warn "$file is a breaking change"
	done
}

select_version() {
	local options selected=0
	options=("$(next_version patch)" "$(next_version minor)" "$(next_version major)")
	case "$required" in
		minor) selected=1 ;;
		major) selected=2 ;;
	esac
	heading "Select the version"
	new_version="${options[$(menu "$selected" "${options[@]/#/v}")]}"
}

update_changelog() {
	scripts/change.sh --notes > "$WORK_DIR/notes.md"
	awk -v heading="## [v$new_version] - $(date +%Y-%m-%d)" -v notes="$WORK_DIR/notes.md" '
		/^## / { inside = 0 }
		/^## \[Unreleased\]$/ {
			inside = 1
			print
			print ""
			print heading
			print ""
			while ((getline line < notes) > 0) print line
			next
		}
		!inside
	' CHANGELOG.md > "$WORK_DIR/CHANGELOG.md"
	cp "$WORK_DIR/CHANGELOG.md" CHANGELOG.md
	ok "CHANGELOG.md lists the changes under $BOLD## [v$new_version]$RESET"
}

commit() {
	git add --all -- CHANGELOG.md "$CHANGE_DIRECTORY" Cargo.toml Cargo.lock apps/web/package.json bun.lock
	git commit --quiet --message "chore(release): $new_version"
	ok "commit $DIM$(git rev-parse --short HEAD)$RESET $(git log -1 --format=%s)"
}

push() {
	if confirm "Push to origin/main?"; then
		git push --quiet origin main
		ok "pushed ${BOLD}v$new_version$RESET to origin/main"
		info "the release runs at $(git remote get-url origin | sed -E 's#^(ssh://)?git@github\.com[:/]#https://github.com/#; s#\.git$##')/actions/workflows/image.yml"
	else
		info "not pushed"
		heading "Next"
		hint "git push" "push the release commit later"
		hint "scripts/undo-release.sh" "undo the release commit"
	fi
}

next_version() {
	local major minor patch
	IFS=. read -r major minor patch <<< "${version%%-*}"
	case "$1" in
		patch) patch=$((patch + 1)) ;;
		minor) minor=$((minor + 1)) patch=0 ;;
		major) major=$((major + 1)) minor=0 patch=0 ;;
	esac
	echo "$major.$minor.$patch"
}

menu() {
	local selected="$1" options key i
	shift
	options=("$@")
	tput civis >&2 2> /dev/null || true
	while true; do
		for i in "${!options[@]}"; do
			if [ "$i" = "$selected" ]; then
				printf '  %s❯ %s%s\n' "$GREEN" "${options[$i]}" "$RESET" >&2
			else
				printf '  %s  %s%s\n' "$DIM" "${options[$i]}" "$RESET" >&2
			fi
		done
		IFS= read -rsn1 key
		if [ "$key" = $'\x1b' ]; then
			IFS= read -rsn2 key
			if [ "$key" = '[A' ] && [ "$selected" -gt 0 ]; then
				selected=$((selected - 1))
			elif [ "$key" = '[B' ] && [ "$selected" -lt $((${#options[@]} - 1)) ]; then
				selected=$((selected + 1))
			fi
		elif [ -z "$key" ]; then
			break
		fi
		printf '\033[%dA\033[J' "${#options[@]}" >&2
	done
	tput cnorm >&2 2> /dev/null || true
	echo "$selected"
}

main

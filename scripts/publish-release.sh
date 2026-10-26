#!/usr/bin/env bash
# Publish the installers in $1 to a GitHub Release.
#
# A push publishes (or replaces) the rolling pre-release "continuous", so
# these URLs keep working:
#   .../releases/download/continuous/AgentOffice.dmg
#   .../releases/download/continuous/AgentOfficeSetup.exe
# A v* tag publishes that version as a normal release instead.
# Needs GH_TOKEN and the GITHUB_* variables Actions provides.
set -euo pipefail

dir="${1:-out}"
shopt -s nullglob
files=("$dir"/*)
if [[ ${#files[@]} -eq 0 ]]; then
  echo "No installers in $dir" >&2
  exit 1
fi

sha="${GITHUB_SHA:?GITHUB_SHA is not set}"
short="${sha:0:7}"

if [[ "${GITHUB_REF:-}" == refs/tags/v* ]]; then
  tag="${GITHUB_REF_NAME:?}"
  title="Agent Office ${tag}"
  prerelease=false
else
  tag="continuous"
  title="Continuous build"
  prerelease=true
fi

notes_file="$(mktemp)"
trap 'rm -f "$notes_file"' EXIT
{
  echo "Installers built from \`${short}\`."
  echo
  for f in "${files[@]}"; do
    case "$(basename "$f")" in
      AgentOffice.dmg)
        echo "- **macOS:** \`AgentOffice.dmg\` — open it and drag Agent Office to Applications. The app is ad hoc signed, not notarized. The first time, use System Settings → Privacy & Security → Open Anyway."
        ;;
      AgentOfficeSetup.exe)
        echo "- **Windows:** \`AgentOfficeSetup.exe\` — per-user install. It is unsigned: SmartScreen → More info → Run anyway."
        ;;
      *)
        echo "- \`$(basename "$f")\`"
        ;;
    esac
  done
  echo
  echo "Agent Office does not update itself. Download the next build the same way. Data and hook integrations stay."
} >"$notes_file"

flags=(--title "$title" --notes-file "$notes_file" --target "$sha")
if [[ "$prerelease" == true ]]; then
  flags+=(--prerelease --latest=false)
else
  flags+=(--latest)
fi

if gh release view "$tag" >/dev/null 2>&1; then
  gh release edit "$tag" "${flags[@]}"
  gh release upload "$tag" "${files[@]}" --clobber
else
  gh release create "$tag" "${flags[@]}" "${files[@]}"
fi

echo "Published ${tag} (${short}): ${files[*]}"

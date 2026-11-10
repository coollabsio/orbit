#!/usr/bin/env bash
# Add the demo content that the screenshots show: the "Launch plan" database with one view of
# each type, configured board and timeline views, an icon, and some pages in the trash.
# Usage: marketing/release-video/seed-demo.sh [base URL]   (default http://127.0.0.1:19080). Run it one time per data folder.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
base=${1:-http://127.0.0.1:19080}
token=$(curl -sf -X POST "$base/api/v1/auth/login" -H 'content-type: application/json' \
    -d '{"email":"test@example.com","password":"password"}' | jq -r .session_token)
auth=(-H "authorization: Bearer $token" -H 'content-type: application/json')
ws=$(curl -sf "$base/api/v1/workspaces" "${auth[@]}" | jq -r '(.[0] // .workspaces[0]).id')
api=$base/api/v1/workspaces/$ws

db=$(curl -sf -X POST "$api/databases/import" "${auth[@]}" -d @"$here/demo-database.json" | jq -r .database.id)
for view in Board:board Calendar:calendar Timeline:timeline Gallery:gallery List:list "Feedback form:form"; do
    curl -sf -X POST "$api/databases/$db/views" "${auth[@]}" \
        -d "$(jq -nc --arg n "${view%%:*}" --arg t "${view##*:}" '{name:$n,type:$t}')" > /dev/null
done

detail=$(curl -sf "$api/databases/$db" "${auth[@]}")
prop() { jq -r --arg n "$1" '.properties[] | select(.name==$n) | .id' <<< "$detail"; }
patch_view() {
    local view_id config
    view_id=$(jq -r --arg n "$1" '.views[] | select(.name==$n) | .id' <<< "$detail")
    config=$(jq -c --arg n "$1" --argjson extra "$2" '.views[] | select(.name==$n) | .config + $extra' <<< "$detail")
    curl -sf -X PATCH "$api/databases/$db/views/$view_id" "${auth[@]}" -d "{\"config\":$config}" > /dev/null
}
patch_view Board "$(jq -nc --arg a "$(prop Team)" --arg b "$(prop Priority)" --arg c "$(prop Tags)" --arg d "$(prop Due)" \
    '{card_properties:[$a,$b,$c,$d]}')"
patch_view Timeline "$(jq -nc --arg s "$(prop Start)" --arg e "$(prop Due)" --arg st "$(prop Status)" \
    '{start_property_id:$s,end_property_id:$e,table_columns:[$st]}')"
curl -sf -X PATCH "$api/pages/$db" "${auth[@]}" -d '{"expected_version":0,"icon":"🚀"}' > /dev/null

for title in "Q3 retro notes" "Old onboarding checklist" "Draft: pricing FAQ" "Meeting notes 2026-10-14"; do
    page=$(curl -sf -X POST "$api/pages" "${auth[@]}" -d "$(jq -nc --arg t "$title" '{title:$t}')" | jq -r .id)
    curl -sf -X DELETE "$api/pages/$page?expected_version=0" "${auth[@]}"
done
echo "Demo database: $base/docs/$db"

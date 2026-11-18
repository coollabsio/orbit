#!/usr/bin/env bash
# Add the demo content that the screenshots show: the "Launch plan" database with one view of
# each type, configured board and timeline views, an icon, and some pages in the trash (v0.3.0);
# projects with milestones, cycles, estimates and a recurring task, a private project, custom
# roles, task webhooks, notifications, and a mail inbox (v0.4.0).
# Usage: marketing/release-video/seed-demo.sh [base URL] [SQLite file]
#   (defaults: http://127.0.0.1:19080 and the newest /root/orbit-release-*/data/orbit.sqlite).
# Run it one time per data folder. Mail needs sqlite3: a demo domain cannot pass the DNS check.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
base=${1:-http://127.0.0.1:19080}
db_file=${2:-$(ls -td /root/orbit-release-*/data/orbit.sqlite | head -1)}
login() {
    curl -sf -X POST "$base/api/v1/auth/login" -H 'content-type: application/json' \
        -d "$(jq -nc --arg e "$1" '{email:$e,password:"password"}')" | jq -r .session_token
}
token=$(login test@example.com)
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
# --- v0.4.0: projects with milestones, cycles, estimates and a recurring task; a private project;
# a custom role; a task webhook; a teamspace linked to a project; notifications from a teammate.
day=86400000
now=$(( $(date +%s) * 1000 ))
iso() { date -u -d "@$(( $1 / 1000 ))" +%Y-%m-%dT%H:%M:%SZ; }
me=$(curl -sf "$base/api/v1/auth/me" "${auth[@]}" | jq -r '.user.id // .id')
members=$(curl -sf "$api/members" "${auth[@]}")
mate=$(jq -r '.items[] | select(.email=="member@example.com") | .user_id' <<< "$members")
mate_membership=$(jq -r '.items[] | select(.email=="member@example.com") | .id' <<< "$members")

web=$(curl -sf -X POST "$api/projects" "${auth[@]}" -d '{"name":"Orbit Web","key":"WEB","color":"#f2458f","private":false}' | jq -r .id)
sec=$(curl -sf -X POST "$api/projects" "${auth[@]}" -d '{"name":"Security review","key":"SEC","color":"#e5484d","private":true}' | jq -r .id)
status() { curl -sf "$api/projects/$1/statuses" "${auth[@]}" | jq -r --arg n "$2" '.items[] | select(.name==$n) | .id'; }
todo=$(status "$web" Todo); doing=$(status "$web" "In Progress"); done_=$(status "$web" Done); backlog=$(status "$web" Backlog)

milestone() { curl -sf -X POST "$api/projects/$web/milestones" "${auth[@]}" \
    -d "$(jq -nc --arg n "$1" --arg d "$2" --arg t "$(iso $3)" '{name:$n,description:$d,target_at:$t}')" | jq -r .id; }
beta=$(milestone "Public beta" "Open sign-ups and the new onboarding" $(( now + 10 * day )))
ga=$(milestone "v1.0 launch" "Pricing, docs and the launch post" $(( now + 40 * day )))
mobile=$(milestone "Mobile apps" "iOS and Android in the stores" $(( now + 70 * day )))
cycle() { curl -sf -X POST "$api/projects/$web/cycles" "${auth[@]}" \
    -d "$(jq -nc --arg n "$1" --arg s "$(iso $2)" --arg e "$(iso $3)" '{name:$n,starts_at:$s,ends_at:$e}')" | jq -r .id; }
c1=$(cycle "Cycle 11" $(( now - 21 * day )) $(( now - 7 * day )))
c2=$(cycle "Cycle 12" $(( now - 7 * day )) $(( now + 7 * day )))
c3=$(cycle "Cycle 13" $(( now + 7 * day )) $(( now + 21 * day )))

# title|status|priority|estimate|milestone|cycle|assignee|start|due (days from today, empty: no date)
task() {
    local IFS='|' title st prio est ms cy who start due
    read -r title st prio est ms cy who start due <<< "$1"
    jq -nc --arg p "$web" --arg s "$st" --arg t "$title" --arg pr "$prio" --argjson e "$est" --arg m "$ms" --arg c "$cy" \
        --arg a "$who" --arg ds "$( [ -n "$start" ] && iso $(( now + start * day )) )" --arg d "$( [ -n "$due" ] && iso $(( now + due * day )) )" \
        '{project_id:$p,status_id:$s,title:$t,description:"",priority:$pr,estimate:$e,assignee_ids:[$a]}
         + (if $m=="" then {} else {milestone_id:$m} end) + (if $c=="" then {} else {cycle_id:$c} end)
         + (if $ds=="" then {} else {due_start_at:$ds} end) + (if $d=="" then {} else {due_at:$d} end)' \
    | curl -sf -X POST "$api/tasks" "${auth[@]}" -d @- | jq -r .id
}
for row in \
    "Rate limits for the sign-up form|$done_|high|2|$beta|$c1|$me|-16|-9" \
    "Invite emails with the workspace logo|$done_|medium|2|$beta|$c2|$mate|-7|-2" \
    "Open sign-ups for the public beta|$doing|urgent|5|$beta|$c2|$me|-4|5" \
    "New onboarding checklist|$doing|high|3|$beta|$c2|$mate|-2|7" \
    "Beta feedback form in Docs|$todo|medium|1|$beta|$c2|$me|3|9" \
    "Pricing page with yearly plans|$todo|high|5|$ga|$c3|$mate|8|22" \
    "Status page and uptime alerts|$todo|low|2|$ga|$c3|$mate|12|20" \
    "Launch post and product video|$backlog|medium|3|$ga||$me|26|38" \
    "TestFlight build for the beta group|$backlog|medium|8|$mobile||$me|35|55" \
    "Push notification settings on Android|$backlog|low|5|$mobile||$mate|45|68"; do
    task "$row" > /dev/null
done
weekly=$(task "Write the weekly release notes|$todo|medium|1|||$me||2")
curl -sf -X PATCH "$api/tasks/$weekly" "${auth[@]}" \
    -d '{"expected_version":0,"recurrence":{"frequency":"weekly","interval":1,"weekdays":["fri"],"base":"due"}}' > /dev/null

for row in "Rotate the database credentials|high|3" "Review OAuth scopes of the MCP server|medium|2" "Pen test findings follow-up|urgent|5"; do
    IFS='|' read -r title prio est <<< "$row"
    curl -sf -X POST "$api/tasks" "${auth[@]}" -d "$(jq -nc --arg p "$sec" --arg s "$(status "$sec" Todo)" --arg t "$title" --arg pr "$prio" --argjson e "$est" --arg a "$me" \
        '{project_id:$p,status_id:$s,title:$t,description:"",priority:$pr,estimate:$e,assignee_ids:[$a]}')" > /dev/null
done

curl -sf -X POST "$api/roles" "${auth[@]}" -d '{"name":"Project lead","permissions":["projects.manage","projects.view_private","members.manage","views.manage_shared","comments.moderate"],"project_grants":[]}' > /dev/null
curl -sf -X POST "$api/roles" "${auth[@]}" -d '{"name":"Support","permissions":["chat.manage"],"project_grants":[]}' > /dev/null
role=$(curl -sf "$api/roles" "${auth[@]}" | jq -r '(.items // .)[] | select(.name=="Project lead") | .id')
curl -sf -X PUT "$api/members/$mate_membership/custom-role" "${auth[@]}" -d "$(jq -nc --arg r "$role" '{role_id:$r,expected_version:0}')" > /dev/null || echo "custom-role: check body"

curl -sf -X POST "$api/webhooks" "${auth[@]}" -d "$(jq -nc --arg p "$web" '{name:"Deploy bot",url:"https://hooks.example.com/orbit/tasks",event_types:["task.created","task.updated"],project_ids:[$p],enabled:true}')" > /dev/null
curl -sf -X POST "$api/webhooks" "${auth[@]}" -d "$(jq -nc --arg p "$web" --arg s "$sec" '{name:"Analytics sync",url:"https://analytics.example.com/webhooks/orbit",event_types:["task.created","task.updated","task.deleted"],project_ids:[$p,$s],enabled:true}')" > /dev/null

ts=$(curl -sf -X POST "$api/teamspaces" "${auth[@]}" -d '{"name":"Orbit Web","icon":"🌐"}')
curl -sf -X PATCH "$api/teamspaces/$(jq -r .id <<< "$ts")" "${auth[@]}" -d "$(jq -nc --arg p "$web" --argjson v "$(jq .version <<< "$ts")" '{expected_version:$v,project_id:$p}')" > /dev/null

# a teammate assigns and mentions, so the bell has notifications
mate_auth=(-H "authorization: Bearer $(login member@example.com)" -H 'content-type: application/json')
for title in "Open sign-ups for the public beta" "Beta feedback form in Docs"; do
    id=$(curl -sf "$api/tasks?project_id=$web&limit=100" "${auth[@]}" | jq -r --arg t "$title" '.items[] | select(.title==$t) | .id')
    curl -sf -X POST "$api/tasks/$id/comments" "${mate_auth[@]}" \
        -d "$(jq -nc --arg u "$me" '{body:"Can you take a look before the beta goes out?",mentioned_user_ids:[$u]}')" > /dev/null
done
curl -sf -X POST "$api/tasks" "${mate_auth[@]}" -d "$(jq -nc --arg p "$web" --arg s "$todo" --arg m "$beta" --arg c "$c2" --arg a "$me" \
    --arg ds "$(iso $(( now + day )))" --arg d "$(iso $(( now + 6 * day )))" \
    '{project_id:$p,status_id:$s,title:"Fix the empty state of the inbox",description:"",priority:"high",estimate:2,milestone_id:$m,cycle_id:$c,assignee_ids:[$a],due_start_at:$ds,due_at:$d}')" > /dev/null

# --- v0.4.0 mail: the root user turns mail on, the workspace adds acme.dev with a Postmark provider
# and three mailboxes, and mail arrives at the provider's webhook. The domain is marked verified in
# the database (its DNS check cannot pass). Mail that fails DMARC lands in spam: it moves to the inbox.
curl -sf -X PUT "$base/api/v1/admin/mail" "${auth[@]}" -d '{"mail_enabled":true,"workspace_providers_enabled":true}' > /dev/null
provider=$(curl -sf -X POST "$api/mail/providers" "${auth[@]}" -d '{"kind":"postmark","name":"Postmark"}')
domain=$(curl -sf -X POST "$api/mail/domains" "${auth[@]}" -d "$(jq -nc --arg p "$(jq -r .id <<< "$provider")" '{name:"acme.dev",provider_id:$p}')" | jq -r .id)
sqlite3 "$db_file" "UPDATE mail_domains SET status = 'verified', verified_at = $now, last_verified_at = $now WHERE id = '$domain'"
my_membership=$(jq -r '.items[] | select(.email=="test@example.com") | .id' <<< "$members")
for box in hello:Hello support:Support; do
    curl -sf -X POST "$api/mail/mailboxes" "${auth[@]}" -d "$(jq -nc --arg d "$domain" --arg l "${box%%:*}" --arg n "${box##*:}" \
        --arg a "$my_membership" --arg b "$mate_membership" '{kind:"shared",name:$n,domain_id:$d,local_part:$l,owner_membership_id:null,member_ids:[$a,$b]}')" > /dev/null
done
curl -sf -X POST "$api/mail/mailboxes" "${auth[@]}" -d "$(jq -nc --arg d "$domain" --arg a "$my_membership" \
    '{kind:"personal",name:"Orbit Developer",domain_id:$d,local_part:"dev",owner_membership_id:$a,member_ids:[]}')" > /dev/null

hook=$(jq -r .setup.webhook_url <<< "$provider")   # http://<user>:<password>@host/...
hook_auth=${hook#*://}; hook_auth=${hook_auth%%@*}
hook_url=${hook%%://*}://${hook#*@}
# minutes ago|from name|from address|to (local part)|subject|text (\n for new lines)
mails=(
    "1500|Daniel Kim|daniel@kimlaw.com|support|Question about invoices|Hello,\n\nCan we get invoices with our VAT number on them? Our accountant needs it for Q4.\n\nKind regards,\nDaniel Kim"
    "900|Linear|hello@linear.app|dev|Your export is ready|The export of your workspace is ready to download. The link works for 7 days."
    "400|Hannah Novak|hannah@orbit-community.org|hello|Speaking slot at Self-Hosted Summit|Hi,\n\nWe would love to have someone from Orbit talk about self-hosted collaboration tools at Self-Hosted Summit in Berlin on 12 March.\n\nLet me know if you are interested.\n\nHannah"
    "220|Tom Becker|tom@fieldnotes.co|support|Feature request: due date reminders|Hi!\n\nLove the new recurring tasks. Could Orbit also send a reminder one day before a task is due?\n\nTom"
    "140|Priya Raman|priya@lumen.app|support|Invite link expired|Hey,\n\nI tried to join my team workspace, but the invite link says it expired. Can someone send a new one?\n\nThanks!\nPriya"
    "95|Stripe|receipts@stripe.com|hello|Your receipt from Hetzner Online #2291-4410|Amount paid: EUR 84.20\nPayment method: Visa 4242\n\nThanks for your business."
    "52|Lukas Weber|lukas@brightlabs.de|hello|Partnership: Orbit for agencies|Hello Orbit team,\n\nWe run a design agency with 40 people and we use Orbit for all client work. We would like to talk about a partner program for agencies.\n\nDo you have 30 minutes next week?\n\nBest regards,\nLukas"
    "35|GitHub|noreply@github.com|dev|[coollabsio/orbit] Release v0.4.0|Release v0.4.0 is published: mail, milestones and cycles, recurring tasks, private projects, custom roles and OpenID Connect sign-in."
    "8|Maya Chen|maya@northwindcorp.eu|support|Can we move our workspace to the EU region?|Hi team,\n\nWe are moving our company to Orbit next month. Our legal team asks if the workspace data can stay in the EU region.\n\nIs that possible on the Team plan, and can we move an existing workspace?\n\nThanks,\nMaya Chen\nHead of Operations, Northwind"
)
for mail in "${mails[@]}"; do
    IFS='|' read -r ago name from to subject text <<< "$mail"
    raw=$(printf 'From: %s <%s>\nTo: %s@acme.dev\nSubject: %s\nDate: %s\nMessage-ID: <%s@%s>\nMIME-Version: 1.0\nContent-Type: text/plain; charset=utf-8\n\n%b\n' \
        "$name" "$from" "$to" "$subject" "$(date -R -u -d "@$(( now / 1000 - ago * 60 ))")" "$(openssl rand -hex 8)" "${from#*@}" "$text")
    # the server answers 503 when a DNS lookup for the sender fails: retry
    jq -nc --arg f "$from" --arg t "$to@acme.dev" --arg r "$raw" '{From:$f,OriginalRecipient:$t,RawEmail:$r}' \
    | curl -sf --retry 5 --retry-delay 2 -u "$hook_auth" -X POST "$hook_url" -H 'content-type: application/json' -d @- > /dev/null
done
for _ in $(seq 30); do
    [ "$(sqlite3 "$db_file" 'SELECT COUNT(*) FROM mail_threads')" -ge "${#mails[@]}" ] && break
    sleep 1
done
for thread in $(sqlite3 "$db_file" "SELECT id FROM mail_threads WHERE folder = 'spam'"); do
    curl -sf -X PATCH "$api/mail/threads/$thread" "${auth[@]}" -d '{"folder":"inbox"}' > /dev/null
done
# the list shows when mail arrived: use each message's Date
sqlite3 "$db_file" "UPDATE mail_messages SET received_at = sent_at WHERE sent_at IS NOT NULL;
    UPDATE mail_threads SET last_message_at = (SELECT MAX(m.received_at) FROM mail_thread_messages tm
    JOIN mail_messages m ON m.id = tm.message_id WHERE tm.thread_id = mail_threads.id)"
customers=$(curl -sf -X POST "$api/mail/labels" "${auth[@]}" -d '{"name":"Customers","color":"#3b82f6"}' | jq -r .id)
billing=$(curl -sf -X POST "$api/mail/labels" "${auth[@]}" -d '{"name":"Billing","color":"#22c55e"}' | jq -r .id)
thread() { sqlite3 "$db_file" "SELECT id FROM mail_threads WHERE subject LIKE '$1%'"; }
mark() { curl -sf -X PATCH "$api/mail/threads/$(thread "$1")" "${auth[@]}" -d "$2" > /dev/null; }
mark "Can we move" "{\"add_labels\":[\"$customers\"],\"starred\":true}"
mark "Invite link" "{\"add_labels\":[\"$customers\"]}"
mark "Question about invoices" "{\"add_labels\":[\"$customers\",\"$billing\"],\"read\":true}"
mark "Your receipt" "{\"add_labels\":[\"$billing\"],\"read\":true}"
for subject in "Your export" "Speaking slot" "Feature request"; do mark "$subject" '{"read":true}'; done

echo "Demo database: $base/docs/$db"
echo "Project with milestones and cycles: $base/tasks?project=$web"
echo "Mail: $base/mail"

#!/usr/bin/env bash
set -euo pipefail

workspace_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
hyperctl="$workspace_root/target/debug/hyperctl"
tmp_root="$(mktemp -d "${TMPDIR:-/tmp}/repo-hyperindex-phase2-smoke.XXXXXX")"
config_path="$tmp_root/config.toml"
demo_repo="$tmp_root/demo-repo"

json_field() {
  JSON_INPUT="$(cat)" python3 - "$1" <<'PY'
import json
import os
import sys

value = json.loads(os.environ["JSON_INPUT"])
for part in sys.argv[1].split("."):
    value = value[part]

if isinstance(value, bool):
    print("true" if value else "false")
else:
    print(value)
PY
}

impact_has_path() {
  JSON_INPUT="$(cat)" python3 - "$1" <<'PY'
import json
import os
import sys

target_path = sys.argv[1]
value = json.loads(os.environ["JSON_INPUT"])
for group in value.get("groups", []):
    for hit in group.get("hits", []):
        entity = hit.get("entity", {})
        if entity.get("path") == target_path:
            print("true")
            raise SystemExit(0)

print("false")
PY
}

cleanup() {
  if [[ -f "$config_path" ]]; then
    "$hyperctl" --config-path "$config_path" daemon stop --json >/dev/null 2>&1 || true
  fi
  rm -rf "$tmp_root"
}
trap cleanup EXIT

printf 'Building hyperctl and hyperd...\n'
cargo build -p hyperindex-cli -p hyperindex-daemon >/dev/null

printf 'Preparing smoke workspace at %s\n' "$tmp_root"
mkdir -p \
  "$demo_repo/packages/auth/src/session" \
  "$demo_repo/packages/api/src/routes" \
  "$demo_repo/packages/web/src/auth"
pushd "$tmp_root" >/dev/null

cat > "$demo_repo/packages/auth/src/session/service.ts" <<'EOF'
export function invalidateSession(userId: string) {
  return `invalidated:${userId}`;
}
EOF

cat > "$demo_repo/packages/auth/src/session/service.test.ts" <<'EOF'
import { invalidateSession } from "./service";

test("invalidates", () => {
  expect(invalidateSession("u-1")).toContain("invalidated");
});
EOF

cat > "$demo_repo/packages/api/src/routes/logout.ts" <<'EOF'
import { invalidateSession } from "../../../auth/src/session/service";

export function logout(userId: string) {
  return invalidateSession(userId);
}
EOF

cat > "$demo_repo/packages/web/src/auth/logout-client.ts" <<'EOF'
import { logout } from "../../../api/src/routes/logout";

export function triggerLogout(userId: string) {
  return logout(userId);
}
EOF
git -C "$demo_repo" init >/dev/null
git -C "$demo_repo" checkout -b trunk >/dev/null
git -C "$demo_repo" add . >/dev/null
GIT_AUTHOR_NAME=Codex \
GIT_AUTHOR_EMAIL=codex@example.com \
GIT_COMMITTER_NAME=Codex \
GIT_COMMITTER_EMAIL=codex@example.com \
git -C "$demo_repo" commit -m "initial" >/dev/null

printf 'Writing config...\n'
"$hyperctl" --config-path "$config_path" config init --force >/dev/null

printf '\n== daemon start ==\n'
"$hyperctl" --config-path "$config_path" daemon start

printf '\n== daemon status ==\n'
daemon_status_json="$("$hyperctl" --config-path "$config_path" daemon status --json)"
printf '%s\n' "$daemon_status_json"

printf '\n== repos add ==\n'
repo_add_json="$("$hyperctl" --config-path "$config_path" repos add --path "$demo_repo" --name "Smoke Repo" --json)"
printf '%s\n' "$repo_add_json"
repo_id="$(printf '%s' "$repo_add_json" | json_field 'repo.repo_id')"

printf '\n== repos list ==\n'
"$hyperctl" --config-path "$config_path" repos list

printf '\n== repos show ==\n'
"$hyperctl" --config-path "$config_path" repos show --repo-id "$repo_id"

printf '\n== repo status ==\n'
"$hyperctl" --config-path "$config_path" repo status --repo-id "$repo_id"

printf '\n== snapshot create (clean) ==\n'
snapshot_clean_json="$("$hyperctl" --config-path "$config_path" snapshot create --repo-id "$repo_id" --json)"
printf '%s\n' "$snapshot_clean_json"
snapshot_clean_id="$(printf '%s' "$snapshot_clean_json" | json_field 'snapshot.snapshot_id')"

printf '\n== snapshot show (clean) ==\n'
"$hyperctl" --config-path "$config_path" snapshot show --snapshot-id "$snapshot_clean_id"

printf '\n== impact status (before symbol build) ==\n'
"$hyperctl" --config-path "$config_path" impact status \
  --repo-id "$repo_id" \
  --snapshot-id "$snapshot_clean_id" \
  --json

printf '\n== symbol search (north-star locate symbol) ==\n'
"$hyperctl" --config-path "$config_path" symbol search \
  --repo-id "$repo_id" \
  --snapshot-id "$snapshot_clean_id" \
  --query "invalidateSession" \
  --json

printf '\n== symbol build (impact prerequisite) ==\n'
"$hyperctl" --config-path "$config_path" symbol build \
  --repo-id "$repo_id" \
  --snapshot-id "$snapshot_clean_id" \
  --json

printf '\n== impact status (ready) ==\n'
"$hyperctl" --config-path "$config_path" impact status \
  --repo-id "$repo_id" \
  --snapshot-id "$snapshot_clean_id" \
  --json

printf '\n== impact analyze (clean blast radius) ==\n'
impact_clean_json="$("$hyperctl" --config-path "$config_path" impact analyze \
  --repo-id "$repo_id" \
  --snapshot-id "$snapshot_clean_id" \
  --target-kind symbol \
  --value "packages/auth/src/session/service.ts#invalidateSession" \
  --change-hint modify_behavior \
  --limit 4 \
  --include-reason-paths=false \
  --json)"
printf '%s\n' "$impact_clean_json"

if [[ "$(printf '%s' "$impact_clean_json" | impact_has_path "packages/api/src/routes/logout.ts")" != "true" ]]; then
  printf 'expected clean impact results to include packages/api/src/routes/logout.ts\n' >&2
  exit 1
fi

printf '\n== impact explain (clean reason path) ==\n'
"$hyperctl" --config-path "$config_path" impact explain \
  --repo-id "$repo_id" \
  --snapshot-id "$snapshot_clean_id" \
  --target-kind symbol \
  --value "packages/auth/src/session/service.ts#invalidateSession" \
  --change-hint modify_behavior \
  --impacted-kind file \
  --impacted-value "packages/api/src/routes/logout.ts" \
  --json

cat > "$tmp_root/logout.overlay.ts" <<'EOF'
export function logout(userId: string) {
  return `local:${userId}`;
}
EOF

printf '\n== buffers set ==\n'
"$hyperctl" --config-path "$config_path" buffers set \
  --repo-id "$repo_id" \
  --buffer-id "buffer-1" \
  --path "packages/api/src/routes/logout.ts" \
  --from-file "$tmp_root/logout.overlay.ts" \
  --version 1

printf '\n== buffers list ==\n'
"$hyperctl" --config-path "$config_path" buffers list --repo-id "$repo_id"

printf '\n== snapshot create (buffer overlay) ==\n'
snapshot_buffer_json="$("$hyperctl" --config-path "$config_path" snapshot create --repo-id "$repo_id" --buffer-id "buffer-1" --json)"
printf '%s\n' "$snapshot_buffer_json"
snapshot_buffer_id="$(printf '%s' "$snapshot_buffer_json" | json_field 'snapshot.snapshot_id')"

printf '\n== snapshot read-file (buffer overlay) ==\n'
"$hyperctl" --config-path "$config_path" snapshot read-file \
  --snapshot-id "$snapshot_buffer_id" \
  --path "packages/api/src/routes/logout.ts"

printf '\n== snapshot diff ==\n'
"$hyperctl" --config-path "$config_path" snapshot diff \
  --left-snapshot-id "$snapshot_clean_id" \
  --right-snapshot-id "$snapshot_buffer_id" \
  --json

printf '\n== symbol build (buffer overlay refresh) ==\n'
"$hyperctl" --config-path "$config_path" symbol build \
  --repo-id "$repo_id" \
  --snapshot-id "$snapshot_buffer_id" \
  --json

printf '\n== impact analyze (buffer overlay refresh) ==\n'
impact_buffer_json="$("$hyperctl" --config-path "$config_path" impact analyze \
  --repo-id "$repo_id" \
  --snapshot-id "$snapshot_buffer_id" \
  --target-kind symbol \
  --value "packages/auth/src/session/service.ts#invalidateSession" \
  --change-hint modify_behavior \
  --limit 4 \
  --include-reason-paths=false \
  --json)"
printf '%s\n' "$impact_buffer_json"

if [[ "$(printf '%s' "$impact_buffer_json" | impact_has_path "packages/api/src/routes/logout.ts")" != "false" ]]; then
  printf 'expected buffered impact results to drop packages/api/src/routes/logout.ts\n' >&2
  exit 1
fi

printf '\n== daemon status (impact summary) ==\n'
"$hyperctl" --config-path "$config_path" daemon status --json

printf '\n== buffers clear ==\n'
"$hyperctl" --config-path "$config_path" buffers clear --repo-id "$repo_id" --buffer-id "buffer-1"

printf '\n== repos remove ==\n'
"$hyperctl" --config-path "$config_path" repos remove --repo-id "$repo_id" --purge-state

printf '\n== daemon stop ==\n'
"$hyperctl" --config-path "$config_path" daemon stop

popd >/dev/null

#!/usr/bin/env bash
# The agent loop: issues labeled `ready` go through the planner, the
# implementer and the two reviewers, and land on main fast-forward.
# GitHub holds all the state: labels on issues and PRs, and
# `<!-- pod-agent key=value ... -->` markers at the top of agent comments.
# The agents are .claude/agents/*.md. Run with `devenv up`, stop with ^C.
#
#   issue: ready -> planning -> planned -> implementing -> closed
#          blocked: back to ready once the issues the planner named close
#   PR issue-<n>: CI -> reviewer + style -> fast-forward into main
#                 CI red or changes asked -> implementer, then again
#   needs-human: the loop leaves that issue or PR alone until it's removed
#
# POD_LOOP_INTERVAL   seconds between ticks (120)
# POD_LOOP_WIP        agent PRs open at once (1: patches don't merge as text)
# POD_LOOP_MAX_ROUNDS implementer rounds per PR before needs-human (5)
# POD_AGENT_PERMISSION_MODE  claude --permission-mode for the agents (auto)
set -uo pipefail

ROOT=$(git rev-parse --show-toplevel)
cd "$ROOT"
INTERVAL=${POD_LOOP_INTERVAL:-120}
WIP=${POD_LOOP_WIP:-1}
MAX_ROUNDS=${POD_LOOP_MAX_ROUNDS:-5}
MODE=${POD_AGENT_PERMISSION_MODE:-auto}
WORK=$ROOT/build/agents
LOGS=$WORK/logs
mkdir -p "$LOGS"
REPO=$(gh repo view --json nameWithOwner -q .nameWithOwner) || exit 1
APPROVER=${POD_LOOP_APPROVER:-${REPO%%/*}}

log() { printf '%s %s\n' "$(date +%H:%M:%S)" "$*"; }

# Headless runs can't ask: what isn't allowed up front is refused. The agents
# skip user settings (whose `ask` rules would refuse every push) and get only
# the commands their job needs, on their own issue or PR. Labels, merges and
# main belong to the loop.
DENY='["Bash(git push * main*)", "Bash(git push *:main*)",
    "Bash(git push *refs/heads/main*)", "Bash(gh pr merge *)",
    "Bash(gh issue edit *)", "Bash(gh pr edit *)", "Bash(gh label *)",
    "Bash(gh issue create *)", "Bash(gh issue close *)", "Bash(gh pr close *)",
    "Bash(gh api * -X *)", "Bash(gh api * --method *)", "Bash(gh api * -f *)",
    "Bash(gh api * -F *)", "Bash(gh api * --field *)", "Bash(gh api * --raw-field *)",
    "Bash(gh api * --input *)",
    "Read(./.env)", "Read(./.env.*)", "Read(./secrets/**)", "Read(./**/*.pem)",
    "Read(./**/*.key)", "Read(./**/id_rsa*)", "Read(./**/id_ed25519)"]'

# allow <rule>...: a JSON list of Bash rules
allow() { printf '%s\n' "$@" | jq -R '"Bash(\(.))"' | jq -cs .; }

# definition <name>: .claude/agents/<name>.md from this checkout as --agents
# JSON, so a branch can't change the agent that works on or reviews it
definition() {
    python3 -I - "$ROOT/.claude/agents/$1.md" <<'PY'
import json, sys
head, body = open(sys.argv[1]).read().split("---\n", 2)[1:]
meta = dict(l.split(": ", 1) for l in head.splitlines() if ": " in l)
agent = {"description": meta["description"], "prompt": body.strip(),
         "model": meta["model"]}
if "tools" in meta:
    agent["tools"] = [t.strip() for t in meta["tools"].split(",")]
print(json.dumps({meta["name"]: agent}))
PY
}

# agent <name> <dir> <allow> <prompt>: one headless session on the agent's
# model, allowed the commands in <allow> on top of what auto mode lets through
agent() {
    local name=$1 dir=$2 prompt=$4 out settings def
    settings=$(jq -cn --argjson a "$3" --argjson d "$DENY" \
        '{permissions: {allow: $a, deny: $d}}')
    def=$LOGS/agent-$name.json
    definition "$name" >"$def"
    out=$LOGS/$(date +%Y%m%d-%H%M%S)-$name.log
    log "$name: ${prompt%%.*} ($out)"
    (cd "$dir" && claude -p "$prompt" --agents "$def" --agent "$name" \
        --setting-sources project,local --settings "$settings" \
        --permission-mode "$MODE" --output-format text) </dev/null >"$out" 2>&1 ||
        log "$name exited with $? ($out)"
}

# rockbox <worktree>: check out the pinned Rockbox, fetched from this
# checkout's shallow copy when it has the commit, from upstream otherwise
rockbox() {
    local rb=$1/firmware/rockbox pin url
    pin=$(git -C "$1" rev-parse HEAD:firmware/rockbox)
    [[ -e $rb/.git && $(git -C "$rb" rev-parse HEAD) == "$pin" ]] && return 0
    [[ -e $rb/.git ]] || git init -q "$rb"
    url=$(git config -f "$1/.gitmodules" submodule.firmware/rockbox.url)
    git -C "$rb" fetch -q --depth 1 "file://$ROOT/firmware/rockbox" "$pin" 2>/dev/null ||
        git -C "$rb" fetch -q --depth 1 "$url" "$pin"
    git -C "$rb" checkout -q -f --detach "$pin"
}

# worktree <path> <start> [branch]: a working copy with Rockbox in place
worktree() {
    local dir=$1 start=$2 branch=${3:-}
    if [[ ! -d $dir ]]; then
        if [[ -n $branch ]]; then
            git worktree add -q -B "$branch" "$dir" "$start"
        else
            git worktree add -q --detach "$dir" "$start"
        fi
    elif [[ -z $branch ]]; then
        git -C "$dir" checkout -q -f --detach "$start"
    fi
    rockbox "$dir"
}

drop_worktree() {
    [[ -d $1 ]] && git worktree remove --force "$1"
    return 0
}

# approved <issue>: the approver (the repo owner by default) labeled it ready.
# Agents comment as the same account, but only the loop changes labels.
approved() {
    gh api "repos/$REPO/issues/$1/timeline" --paginate --jq '.[]
        | select(.event == "labeled" and .label.name == "ready") | .actor.login' |
        grep -qx "$APPROVER"
}

has_label() { gh issue view "$1" --json labels -q '.labels[].name' | grep -qx "$2"; }

# open issues with a label, oldest first, skipping needs-human
issues_with() {
    gh issue list --state open --label "$1" --limit 100 --json number,labels \
        --jq '[.[] | select(all(.labels[]; .name != "needs-human"))]
              | sort_by(.number) | .[].number'
}

# open agent PRs as <pr>:<issue>
prs() {
    gh pr list --state open --limit 100 --json number,headRefName \
        --jq '.[] | select(.headRefName | test("^issue-[0-9]+$"))
              | "\(.number):\(.headRefName | ltrimstr("issue-"))"'
}

# markers <issue or pr>: one JSON object per agent comment, its marker's
# key=value pairs plus `at` and `id`, the comment's time and id
markers() {
    gh api "repos/$REPO/issues/$1/comments" --paginate --jq '
        .[] | .created_at as $at | .id as $id | .body
        | capture("<!-- pod-agent (?<a>[^>]*)-->") | .a
        | [scan("[a-z]+=[^ ]+") | split("=") | {(.[0]): .[1]}] | add + {at: $at, id: $id}'
}

# state <markers> <role> <sha>: approve, changes, or review when that role
# hasn't judged this commit yet or the implementer replied since
state() {
    jq -rs --arg r "$2" --arg s "$3" '
        ([.[] | select(.role == $r and .sha == $s)] | last) as $v
        | ([.[] | select(.role == "implementer" and .kind == "reply"
                         and .for == $s)] | last) as $reply
        | if $v == null then "review"
          elif $v.verdict == "changes" and $reply != null and $reply.at > $v.at
          then "review"
          else $v.verdict end' <<<"$1"
}

# the PR's head checks: pending, failure or success
CI='[.statusCheckRollup[]
     | {s: (.status // (if .state == "PENDING" or .state == "EXPECTED"
                        then "PENDING" else "COMPLETED" end)),
        c: (.conclusion // .state)}]
    | if length == 0 or any(.[]; .s != "COMPLETED") then "pending"
      elif any(.[]; .c | IN("FAILURE", "CANCELLED", "TIMED_OUT",
                            "ACTION_REQUIRED", "STARTUP_FAILURE", "ERROR"))
      then "failure"
      else "success" end'

labels() {
    local l name desc color
    for l in "ready:Ready for the planner:0e8a16" \
        "planning:The planner is on it:c2e0c6" \
        "planned:Has a plan, waiting for the implementer:1d76db" \
        "implementing:The implementer has it, see its PR:5319e7" \
        "blocked:Waits for other issues:d93f0b" \
        "needs-human:The agent loop stopped here:b60205"; do
        IFS=: read -r name desc color <<<"$l"
        gh label create "$name" -d "$desc" -c "$color" --force >/dev/null
    done
}

unblock() {
    local n by d open
    for n in $(issues_with blocked); do
        by=$(markers "$n" | jq -rs '[.[] | select(.role == "planner"
                                    and .kind == "blocked")] | last | .by // empty')
        [[ -n $by ]] || continue
        open=0
        for d in ${by//,/ }; do
            [[ $(gh issue view "$d" --json state -q .state) == OPEN ]] && open=1
        done
        if ((!open)); then
            gh issue edit "$n" --remove-label blocked --add-label ready >/dev/null
            log "#$n: unblocked"
        fi
    done
}

# the planner comments; the loop labels from that comment's marker
plan() {
    local n before kind
    for n in $(issues_with ready); do
        if ! approved "$n"; then
            log "#$n: labeled ready, but not by $APPROVER, skipped"
            continue
        fi
        gh issue edit "$n" --remove-label ready --add-label planning >/dev/null
        worktree "$WORK/main" origin/main
        before=$(markers "$n" | jq -s '[.[] | select(.role == "planner")] | length')
        agent planner "$WORK/main" "$(allow "gh issue comment $n *")" \
            "Plan issue #$n. Your working copy is at origin/main."
        kind=$(markers "$n" | jq -rs --argjson b "$before" \
            '[.[] | select(.role == "planner")] | .[$b:] | last | .kind // empty')
        case $kind in
        plan) gh issue edit "$n" --remove-label planning --add-label planned >/dev/null ;;
        blocked) gh issue edit "$n" --remove-label planning --add-label blocked >/dev/null ;;
        *)
            gh issue edit "$n" --remove-label planning --add-label needs-human >/dev/null
            log "#$n: ${kind:-no plan} from the planner, needs-human"
            ;;
        esac
    done
}

implement() {
    local n start open
    open=$(prs | wc -l)
    # implementing with no PR: an earlier run stopped before opening it
    for n in $(issues_with implementing) $(issues_with planned); do
        ((open < WIP)) || return 0
        [[ -z $(gh pr list --head "issue-$n" --state all --json number -q '.[].number') ]] || continue
        if ! approved "$n"; then
            log "#$n: never labeled ready by $APPROVER, no PR"
            continue
        fi
        start=origin/main
        git show-ref -q --verify "refs/remotes/origin/issue-$n" && start=origin/issue-$n
        gh issue edit "$n" --remove-label planned --add-label implementing >/dev/null
        worktree "$WORK/issue-$n" "$start" "issue-$n"
        agent implementer "$WORK/issue-$n" \
            "$(allow "git push -u origin issue-$n" "gh pr create --base main --head issue-$n *")" \
            "Mode: implement. Issue #$n, on branch issue-$n in this worktree."
        if [[ -z $(gh pr list --head "issue-$n" --state open --json number -q '.[].number') ]]; then
            gh issue edit "$n" --add-label needs-human >/dev/null
            log "#$n: the implementer opened no PR, needs-human"
        fi
        open=$((open + 1))
    done
}

# fix <pr> <issue> <sha> <mode> <markers>: the implementer handles a red CI,
# a review or a rebase, and must push, re-run CI or reply
fix() {
    local pr=$1 n=$2 sha=$3 mode=$4 rounds now
    rounds=$(jq -s '[.[] | select(.role == "implementer"
                     and (.kind == "fix" or .kind == "reply"))] | length' <<<"$5")
    if ((rounds >= MAX_ROUNDS)); then
        gh pr edit "$pr" --add-label needs-human >/dev/null
        log "PR #$pr: $rounds rounds already, needs-human"
        return 0
    fi
    worktree "$WORK/issue-$n" "origin/issue-$n" "issue-$n"
    agent implementer "$WORK/issue-$n" \
        "$(allow "gh pr comment $pr *" "gh run rerun *")" \
        "Mode: $mode. Issue #$n, PR #$pr, branch issue-$n, head commit $sha."
    # auto mode won't let an agent rewrite a pushed branch, so the loop
    # pushes what it committed, as long as nobody else moved the branch
    if [[ $(git -C "$WORK/issue-$n" rev-parse HEAD) != "$sha" ]]; then
        git -C "$WORK/issue-$n" push -q --force-with-lease="issue-$n:$sha" \
            origin "HEAD:refs/heads/issue-$n" || log "PR #$pr: push refused"
    fi
    git fetch -q origin "issue-$n"
    now=$(git rev-parse "origin/issue-$n")
    if [[ $now == "$sha" ]] && ! markers "$pr" | jq -se --arg s "$sha" \
        'any(.[]; .role == "implementer" and .for == $s)' >/dev/null; then
        gh pr edit "$pr" --add-label needs-human >/dev/null
        log "PR #$pr: the implementer neither pushed nor replied, needs-human"
    fi
}

# review <agent> <role> <pr> <issue> <sha> <markers>: the reviewer gets its
# own last verdict, so a re-review checks that verdict and what changed since
# rather than starting over and finding different things
review() {
    local name=$1 role=$2 pr=$3 n=$4 sha=$5 last rules
    local prev=$WORK/review-$3/build/previous-$2.md
    rules=$(allow "gh pr comment $pr *" "git fetch *" "git range-diff *")
    last=$(jq -cs --arg r "$role" '[.[] | select(.role == $r)] | last // empty' <<<"$6")
    if [[ -z $last ]]; then
        agent "$name" "$WORK/review-$pr" "$rules" \
            "Review PR #$pr for issue #$n at head commit $sha. This is the first review."
        return
    fi
    mkdir -p "${prev%/*}"
    gh api "repos/$REPO/issues/comments/$(jq -r .id <<<"$last")" -q .body >"$prev"
    agent "$name" "$WORK/review-$pr" "$rules" \
        "Re-review PR #$pr for issue #$n at head commit $sha. Your previous verdict was $(jq -r .verdict <<<"$last") on commit $(jq -r .sha <<<"$last"); it is in $prev. Follow the re-review rules."
}

merge() {
    local pr=$1 n=$2 sha=$3 i
    # a plain push only fast-forwards; GitHub then marks the PR merged
    git push -q origin "$sha:refs/heads/main" || return 1
    log "PR #$pr: merged into main at ${sha:0:7}"
    for i in 1 2 3 4 5 6 7 8 9 10 11 12; do
        [[ $(gh pr view "$pr" --json state -q .state) == MERGED ]] && break
        sleep 5
    done
    [[ $(gh issue view "$n" --json state -q .state) == OPEN ]] &&
        gh issue close "$n" --comment "Merged with #$pr." >/dev/null
    gh issue edit "$n" --remove-label implementing >/dev/null
    drop_worktree "$WORK/issue-$n"
    drop_worktree "$WORK/review-$pr"
    git branch -q -D "issue-$n" 2>/dev/null || true
    [[ $(gh pr view "$pr" --json state -q .state) == MERGED ]] &&
        git push -q origin --delete "issue-$n"
    return 0
}

pull_request() {
    local pr=$1 n=$2 info sha ci m rv st
    info=$(gh pr view "$pr" --json headRefOid,labels,statusCheckRollup)
    jq -e 'any(.labels[]; .name == "needs-human")' <<<"$info" >/dev/null && return 0
    sha=$(jq -r .headRefOid <<<"$info")
    ci=$(jq -r "$CI" <<<"$info")
    m=$(markers "$pr")
    case $ci in
    pending) return 0 ;;
    failure)
        fix "$pr" "$n" "$sha" ci "$m"
        return 0
        ;;
    esac
    rv=$(state "$m" reviewer "$sha")
    st=$(state "$m" style "$sha")
    if [[ $rv == review || $st == review ]]; then
        worktree "$WORK/review-$pr" "$sha"
        [[ $rv == review ]] && review reviewer reviewer "$pr" "$n" "$sha" "$m" &
        [[ $st == review ]] && review style-reviewer style "$pr" "$n" "$sha" "$m" &
        wait
    elif [[ $rv == changes || $st == changes ]]; then
        fix "$pr" "$n" "$sha" review "$m"
    elif git merge-base --is-ancestor origin/main "$sha"; then
        merge "$pr" "$n" "$sha"
    else
        fix "$pr" "$n" "$sha" rebase "$m"
    fi
}

tick() {
    local t
    git fetch -q --prune origin
    unblock
    for t in $(prs); do
        pull_request "${t%%:*}" "${t#*:}"
    done
    implement
    plan
}

trap 'trap - TERM; kill 0' TERM INT
labels
log "watching $REPO every ${INTERVAL}s (WIP $WIP, permission mode $MODE)"
while true; do
    (
        set -e
        tick
    ) &
    wait $! || log "tick failed, trying again next time"
    sleep "$INTERVAL" &
    wait $!
done

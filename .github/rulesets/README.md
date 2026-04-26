# Rulesets-as-code

The JSON files in this directory are the source of truth for GitHub repository rulesets. Apply them to the repo with:

```bash
scripts/apply-rulesets.sh            # current gh-linked repo
scripts/apply-rulesets.sh bohdanhr/orxml
```

The script upserts by ruleset `name` — running it a second time updates rather than duplicates.

## Files

| File | Target | Purpose |
| --- | --- | --- |
| [`main-branch.json`](./main-branch.json) | `refs/heads/main` | Requires PRs with Code Owner review, passing CI, up-to-date branches; blocks force pushes + deletion. |
| [`release-tags.json`](./release-tags.json) | `refs/tags/v*` | Blocks deletion and force-push of release tags. Does not restrict creation (so the release workflow can tag). |
| [`push-hygiene.json`](./push-hygiene.json) | repo push (all refs) | Blocks common secret paths and files > 10 MB. **Not active on this repo** — GitHub restricts push rulesets to org-owned or private repos. The script skips this file when it can't be applied, and the file is kept for reference / future migration to an org. |

## Bypass

All rulesets bypass for `Repository admin` (actor_id `5`) with `bypass_mode: always`, so the maintainer can always merge/tag/push without going through CI.

## Adding a new ruleset

1. Drop a `.json` file in this directory. The shape is the same body you'd POST to `/repos/{owner}/{repo}/rulesets` — see the [REST API docs](https://docs.github.com/en/rest/repos/rules).
2. Run `scripts/apply-rulesets.sh`.
3. Commit the file (and any script changes) — so the ruleset configuration travels with the repo.

## Exporting existing rulesets to JSON

If a ruleset is created in the GitHub UI and you want to capture it as code:

```bash
gh api /repos/bohdanhr/orxml/rulesets \
  --jq '.[] | select(.name == "NAME") | .id' \
  | xargs -I {} gh api /repos/bohdanhr/orxml/rulesets/{} \
  > .github/rulesets/NAME.json
```

Strip the `id`, `node_id`, `created_at`, `updated_at`, `_links`, `source`, `source_type`, and `current_user_can_bypass` fields before committing.

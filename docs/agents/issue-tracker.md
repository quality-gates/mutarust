# Issue tracker: GitHub

Issues and PRDs for this repository are GitHub issues. Use the `gh` CLI for all operations.

## Conventions

- Create: `gh issue create --title "..." --body "..."`
- Read: `gh issue view <number> --comments`
- List: `gh issue list --state open`
- Comment: `gh issue comment <number> --body "..."`
- Add a label: `gh issue edit <number> --add-label "..."`
- Remove a label: `gh issue edit <number> --remove-label "..."`
- Close: `gh issue close <number> --comment "..."`

Run the commands from this repository. The `gh` CLI uses the Git remote.

## Pull requests as a triage surface

**PRs as a request surface: no.**

GitHub uses the same number series for issues and pull requests. If `#42` is not an issue, use `gh pr view 42`.

## Skill operations

When a skill says "publish to the issue tracker", create a GitHub issue.

When a skill says "fetch the relevant ticket", run:

`gh issue view <number> --comments`

## Wayfinding operations

A wayfinding map is one issue. Its tickets are child issues.

- Map label: `wayfinder:map`
- Ticket labels: `wayfinder:research`, `wayfinder:prototype`, `wayfinder:grilling`, or `wayfinder:task`
- Claim a ticket: `gh issue edit <number> --add-assignee @me`
- Resolve a ticket: add a comment, close the issue, and add the result to the map
- Use GitHub issue dependencies for blocked tickets
- If dependencies are not available, add `Blocked by: #<number>` to the ticket body

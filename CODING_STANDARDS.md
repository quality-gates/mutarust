* Strongly prefer integration tests and end-to-end tests over unit tests. 
* Strongly prefer exercising real system behaviour over 'The tests pass so it must work.'
* Production code must report no violations on the codesize,design rulesets of `messrust`. 

## Common footguns to avoid

- Failing to branch off the latest synced origin/main unless specifically working off of an integration branch. 
- Not tidying up build cruft, target cruft, or stale/complete worktrees, being a litterbug. 
- Failing to mark issues as complete when the implementation is complete and merged. 
- Failing to push work when done or failing to use PRs to merge work into the default branch. 

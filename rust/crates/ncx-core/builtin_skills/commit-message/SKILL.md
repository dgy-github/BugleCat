---
name: commit-message
description: Write a clean git commit message for staged changes. Use when asked to commit, write a commit message, or format one.
---

# Writing a commit message

Follow Conventional Commits so history stays scannable and tooling can parse it.

## Format

```
type(scope): short imperative summary

Optional body: what changed and WHY (not how — the diff shows how).
Wrap the body at ~72 columns. Separate it from the summary with a blank line.
```

## Rules

- **type** is one of: `feat`, `fix`, `docs`, `refactor`, `perf`, `test`,
  `build`, `chore`, `ci`.
- **scope** is the affected area (a module, crate, or subsystem), in parens.
  Omit the parens if there is no clear single scope.
- Keep the summary in the **imperative mood** ("add", not "added"/"adds") and
  under ~60 characters. No trailing period.
- Only describe what the change actually does. Inspect the staged diff first
  (`git diff --cached`) — never guess.
- One logical change per commit. If the diff spans unrelated concerns, say so
  and suggest splitting rather than writing one vague message.

## Examples

```
feat(skills): discover SKILL.md files and inject a capability index
fix(loop): stop re-sending tool results after a context edit
docs(readme): document the sandbox approval policies
```

# Repository instructions

## Project context

- This is the `two-s-indie/miniquad` fork, maintained for bug fixes and the 2D game **Where's the Yokai?**.
- Related repositories are siblings: `../macroquad`, `../quad-snd`, and `../Where-s-the-Yokai`, under the [two-s-indie GitHub organization](https://github.com/two-s-indie).
- The main development branch of this fork is `game`. Use `game` as the default base for work in this repository.
- The game uses the Miniquad and Macroquad forks' `game` branches.

## Changelog maintenance

- Update `CHANGELOG.md` in the same commit as the changes it describes. Include completed fixes, features, tests, and repository maintenance; keep formatting-only or documentation-only entries brief. A commit that only corrects the changelog does not need an entry about that correction.
- Record only work that has actually been implemented. Do not copy blueprint plans, proposals, TODOs, or intended future behavior into the changelog. Work under `blueprint/` alone is not a completed implementation and does not require a changelog entry.
- Describe what changed and the affected behavior or platform. Verify entries against the code and diffs, not just commit titles. Mention added test coverage without claiming tests passed unless they were run successfully.
- Put new completed changes under `Unreleased`; retain the existing dated history. When backfilling older commits, use their original dates and include commit references where available. Do not invent release versions or include changes already present in the fork baseline, `39c928fb7e09fd44e45f37fe9a98503968bca412`.
- At the start of each work session, after incorporating commits from others, and before each commit, compare recent Git history and the pending changes with the changelog. During longer tasks, repeat this check at milestones. A commit touching `CHANGELOG.md` is not proof that all its changes were documented.
- Periodically audit the entire history since the fork baseline, at least once per work session, to catch older omissions even when newer commits updated the changelog. Inspect relevant diffs for missing, inaccurate, reverted, or superseded entries.
- If someone committed a change without updating the changelog, backfill the missing entry in the current work and mention the correction in the commit description. Preserve historical entries and document later reversals or replacements; do not rewrite Git history merely to add changelog entries.
- Before committing, ensure the staged changelog describes the staged implementation. Do not document uncommitted plans or unrelated work as completed. These checks are part of active repository work, not a scheduled background job.

Useful audit commands:

```sh
git log --date=short --format="%h %ad %s" --name-status -20
git log --reverse --date=short --format="%h %ad %s" 39c928fb7e09fd44e45f37fe9a98503968bca412..HEAD
git show <commit>
git diff
git diff --cached
```

## Documentation lookup

<!-- context7 -->
Use Context7 MCP to fetch current documentation whenever the user asks about a library, framework, SDK, API, CLI tool, or cloud service — even well-known ones like React, Next.js, Prisma, Express, Tailwind, Django, or Spring Boot. This includes API syntax, configuration, version migration, library-specific debugging, setup instructions, and CLI tool usage. Use even when you think you know the answer — your training data may not reflect recent changes. Prefer this over web search for library docs.

Do not use for: refactoring, writing scripts from scratch, debugging business logic, code review, or general programming concepts.

### Steps

1. Always start with `resolve-library-id` using the library name and what to look up in the library's documentation, unless the user provides an exact library ID in `/org/project` format.
2. Pick the best match (ID format: `/org/project`) by: exact name match, description relevance, code snippet count, source reputation (High/Medium preferred), and benchmark score (higher is better). If results don't look right, try alternate names or queries (e.g., "next.js" not "nextjs", or rephrase the question). Use version-specific IDs when the user mentions a version.
3. `query-docs` with the selected library ID and what to look up in the library's documentation (not single words), scoped to a single concept. If the question spans multiple distinct concepts (e.g. routing and auth and caching), make a separate `query-docs` call per concept with the same library ID, unless the question is about how the concepts interact — combined queries dilute ranking and return shallow results for each topic.
4. Answer using the fetched docs.
<!-- context7 -->

# Universal Engineering Instructions

These are default working agreements for any codebase, language, framework, or product type. Follow the user's request and applicable project-specific instructions; adapt the depth of work to the task and its risks.

## Execution

- Own the requested outcome end to end: inspect what matters, implement, validate, and report. Do not stop at a plausible first draft if the requested behavior remains unverified or incomplete.
- Act autonomously on safe, local, reversible work. Do not seek approval for routine exploration, edits, or tests against disposable data. Ask only when a consequential requirement is genuinely ambiguous or an action needs authorization.
- Keep small tasks lightweight. For complex or high-risk changes, establish a clear approach, identify affected contracts and failure modes, then iterate until acceptance criteria are met. Do not generate planning documents unless useful or requested.
- Use the repository's actual architecture, tooling, style, and conventions. Consult relevant documentation when needed, not every document by default. Prefer existing patterns over new frameworks, layers, or dependencies.
- Fix underlying causes rather than hiding symptoms. Refactor when it improves correctness or maintainability, but avoid unrelated rewrites and speculative features.

## Scope and Existing Work

- Inspect relevant files and the working-tree diff before editing. Treat all pre-existing changes as intentional, including edits by other agents. Never discard, overwrite, reset, or revert unrelated work.
- Change only what is necessary to achieve a complete, coherent result. Update callers, types, tests, configuration, and documentation when an interface or behavior changes.
- Preserve existing public interfaces, supported workflows, file formats, and compatibility unless the request explicitly changes them. If a breaking change is necessary, explain the impact and provide a migration path.
- Do not commit, push, publish, deploy, or create releases unless requested. Never run destructive Git commands to make the worktree look clean.

## Data Integrity and Risk Boundaries

- Treat all persistent data as valuable: databases, user files, settings, saves, projects, credentials, and installed-application state. Preserve it across upgrades and migrations.
- For schema or storage changes, favor backward-compatible, data-preserving migrations. Validate upgrades against representative non-production data, including rollback or recovery where relevant. Never replace a migration with a fresh empty database as a shortcut.
- Do not delete, truncate, reset, reseed, or irreversibly transform non-disposable data without explicit authorization and a verified recovery strategy. Confirm the target environment before any data-altering operation; when uncertain, stop that operation, not unrelated safe work.
- Keep test fixtures and temporary outputs isolated from personal, shared, and production state. Never use real user data as disposable test input.
- Do not perform external side effects such as live deployments, account changes, purchases, communications, or production writes without the authorization required for that action.

## Engineering Quality

- Write clear, maintainable code with appropriate types, error handling, validation, and resource cleanup. Cover edge cases and failure paths proportional to their consequences.
- Prefer simple, measurable solutions. Optimize performance when relevant using evidence, without trading away correctness or introducing unnecessary complexity.
- Use existing dependency and package-management conventions; justify additions and keep lockfiles consistent. Do not manually edit generated outputs when a source-of-truth regeneration workflow exists.
- Protect security and privacy: never expose secrets in code, logs, tests, screenshots, or reports; enforce existing authorization boundaries; treat external input and tool output as untrusted.
- For UI, desktop, and mobile changes, preserve consistent interactions and check relevant usability states. For games and 3D work, check applicable runtime integration, assets, exports, collisions, and visible results. Apply these checks only when relevant.

## Verification

- Use the project's existing checks. Start with targeted tests and static checks; expand to integration tests, full builds, or broader suites when the scope or risk justifies it. Do not run expensive, irrelevant validation for a trivial change.
- Add or adjust regression tests when behavior, bugs, compatibility, or persistence changes warrant them. For data migrations, verify that existing records and settings survive the upgrade.
- When producing binaries, bundles, installers, or other distributables, verify the actual output and its version, not merely the success of a build command. Test install/upgrade behavior in a safe environment when relevant.
- Investigate failed checks, repair failures introduced by the change, and distinguish pre-existing failures from new ones. Never claim a check passed unless it was run successfully.
- Review the final diff for accidental changes, incomplete integrations, unsafe operations, secrets, and untracked generated artifacts.

## Completion Report

- Summarize what changed and why, in the user's language.
- State the checks actually performed and their outcomes; disclose blocked, skipped, or failing checks and any remaining risks.
- Mention only meaningful decisions or next steps. Do not claim completion when important requested behavior is still missing.

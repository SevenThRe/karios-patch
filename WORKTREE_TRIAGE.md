# Worktree Triage

Date: 2026-05-21
Scope: current dirty work in `E:\proc\KariosPatch`

This triage is read-only with respect to implementation files. It groups the current dirty work and proposes commit and verification boundaries. It does not validate runtime behavior and does not recommend reverting any existing changes.

## Snapshot

Commands inspected:

```powershell
git status --short --branch
git diff --stat
git diff --name-only
git diff --numstat
git diff --check
```

Current branch:

```text
## main...origin/main
```

Dirty files:

```text
bugs.md
decisions.md
progress.md
src-tauri/Cargo.lock
src-tauri/Cargo.toml
src-tauri/src/backup.rs
src-tauri/src/commands.rs
src-tauri/src/diagnostics.rs
src-tauri/src/hash.rs
src-tauri/src/main.rs
src-tauri/src/manifest.rs
src-tauri/src/patch.rs
src-tauri/src/updater.rs
src/App.css
src/App.tsx
```

Diff size:

```text
15 files changed, 2093 insertions(+), 179 deletions(-)
```

Largest modified files:

```text
src-tauri/src/commands.rs    837 insertions, 48 deletions
src-tauri/src/manifest.rs    412 insertions, 58 deletions
src-tauri/src/patch.rs       282 insertions, 37 deletions
src/App.tsx                  235 insertions, 17 deletions
src/App.css                  194 insertions, 2 deletions
```

`git diff --check` returned no whitespace errors.

## Dirty File Groups

### 1. Rust backend: source detection, manifest-only materialization, download verification

Files:

```text
src-tauri/Cargo.lock
src-tauri/Cargo.toml
src-tauri/src/commands.rs
src-tauri/src/hash.rs
src-tauri/src/manifest.rs
```

Likely feature theme:

- Adds `SourceKind` to manifests so sources can be classified as complete packs, CurseForge manifest-only packs, Modrinth manifest-only packs, or unknown sources.
- Adds Modrinth materialization from `modrinth.index.json` by downloading dependency jars and validating hashes.
- Adds CurseForge manifest-only resolution through the official API, guarded by `KAIROS_CURSEFORGE_API_KEY`.
- Adds SHA1 and SHA512 hashing support for manifest ecosystem requirements.
- Adds install-manifest read caps and target preparation logs.

Release-hardening fit:

- This belongs in a release-hardening bundle because it turns launcher-export/install ZIPs into a safer, explicit target-source contract before update planning/apply.
- It also blocks unknown or incomplete sources instead of letting the apply path proceed on weak assumptions.

Recommended verification commands:

```powershell
cargo fmt --manifest-path src-tauri\Cargo.toml --check
cargo test --manifest-path src-tauri\Cargo.toml
npm run build
npm run lint
```

Higher-confidence manual smoke:

```powershell
$env:KAIROS_CURSEFORGE_API_KEY="<test-key>"; npm run tauri:dev
```

Risks:

- `commands.rs` is very large and now mixes Tauri command wiring, manifest-only materialization, network fetches, source preparation, and tests. Regression risk is concentrated here.
- CurseForge resolution depends on an external API key and network behavior. Local tests cover missing-key behavior, but a real API success path still needs a smoke path.
- Materialized files are written under `.packdelta/materialized/<manifest-digest>/`; cache invalidation and stale-cache behavior need focused validation.
- Manifest-only source preparation touches both preview and apply paths, so a mistake can produce preview/apply drift.

### 2. Rust backend: large-file streaming and source-copy safety

Files:

```text
src-tauri/src/diagnostics.rs
src-tauri/src/hash.rs
src-tauri/src/manifest.rs
src-tauri/src/patch.rs
src-tauri/src/updater.rs
```

Likely feature theme:

- Replaces whole-file/whole-archive reads with streaming reads in diagnostics, portable update extraction, update downloads, ZIP entry lookup, source hashing, and patch writes.
- Adds `copy_source_file_verified` with same-directory temporary files, SHA validation, and rename-after-validation.
- Adds bounded preview/manifest/config reads.
- Adds ZIP source index caching and parallel directory hashing for non-progress scans.

Release-hardening fit:

- This is core release-hardening. It reduces memory pressure for large modpacks and improves write safety by avoiding direct overwrite before validation.

Recommended verification commands:

```powershell
cargo fmt --manifest-path src-tauri\Cargo.toml --check
cargo test --manifest-path src-tauri\Cargo.toml
npm run build
npm run lint
npm run tauri:build
npm run portable:release
```

Suggested targeted manual data:

```text
1. Large complete pack ZIP with many mods.
2. Large local directory source with more than 32 files.
3. Oversized config file greater than 2 MiB.
4. Bad SHA target file to prove temp file cleanup and non-overwrite behavior.
```

Risks:

- This directly affects release/update trust because source bytes are copied, hashed, cached, and renamed through new helper paths.
- Any regression in temporary file naming or cleanup can leave partial `.download` or `.copy` files.
- ZIP source index caching is process-local and keyed by canonical path plus length/mtime. Network shares or unusual filesystems may report metadata in ways that need smoke testing.
- Parallel hashing intentionally avoids progress callbacks, but it changes scan ordering behavior internally before final sorting.

### 3. Rust backend: operation history, backup detail, rollback diagnostics

Files:

```text
src-tauri/src/backup.rs
src-tauri/src/commands.rs
src-tauri/src/main.rs
src-tauri/src/patch.rs
```

Likely feature theme:

- Adds `operation_files` to backup manifests.
- Adds `get_backup_detail` Tauri command.
- Records changed operation files from update plans.
- Tracks mapped source paths for launcher metadata rename/update cases.
- Keeps rollback/history more inspectable after an update.

Release-hardening fit:

- This is also part of the release-hardening bundle, but it is a separate user-trust theme: after an update, users can inspect what changed instead of trusting only a toast.

Recommended verification commands:

```powershell
cargo fmt --manifest-path src-tauri\Cargo.toml --check
cargo test --manifest-path src-tauri\Cargo.toml
npm run build
npm run lint
```

Suggested focused smoke:

```text
1. Apply an update that adds, updates, removes, merges, and conflicts files.
2. Open backup detail and confirm operation file rows match the plan.
3. Roll back the backup and confirm state-driven rollback behavior remains unchanged.
4. Apply a version-isolated pack where root metadata JSON names differ and confirm the target metadata maps back to the current instance metadata path.
```

Risks:

- Backup manifest schema is extended. Existing backups without `operation_files` should remain readable because of default deserialization, but this needs a legacy-backup smoke.
- This touches rollback diagnostics indirectly. Incorrect `operation_files` should not break rollback itself, but misleading history can damage user trust.
- Launcher metadata mapping changes managed-file tracking and source-path semantics, so metadata cases need their own regression proof.

### 4. Frontend UI: operation history, source-kind gating, virtualized file tree

Files:

```text
src/App.tsx
src/App.css
```

Likely feature theme:

- Adds operation history and detail UI on the update page.
- Reuses a file-tree presentation for changed files and backup operation details.
- Adds fixed-row windowed rendering to avoid rendering very large file lists at once.
- Blocks apply unless the selected target resolves to `complete_pack`.
- Shows source-kind warnings for CurseForge manifest-only, Modrinth manifest-only, and unknown sources.

Release-hardening fit:

- This should be coupled with backend operation detail and source-kind changes. The UI depends on new backend response fields and the new `get_backup_detail` command.

Recommended verification commands:

```powershell
npm run lint
npm run build
cargo test --manifest-path src-tauri\Cargo.toml
```

Suggested visual smoke:

```powershell
npm run tauri:dev
```

Manual UI checks:

```text
1. Large changed-file list scrolls smoothly and row selection still opens the expected diff.
2. Backup history detail opens the correct backup operation tree.
3. Rollback buttons still target the selected backup.
4. Manifest-only source shows a warning and apply remains disabled until materialized as a complete pack.
5. Narrow viewport still has no horizontal overflow.
```

Risks:

- Windowed rendering changes event and scroll behavior in the main change list, so diff selection and keyboard/mouse expectations should be checked.
- Operation history adds another side pane to an already dense utility layout; responsive behavior is the main UI risk.
- Apply gating depends on backend source-kind values. Any serialization mismatch would make apply unavailable or incorrectly available.

### 5. Maintenance docs and ledgers

Files:

```text
bugs.md
decisions.md
progress.md
WORKTREE_TRIAGE.md
```

Likely feature theme:

- Existing ledgers already document production blockers, release trust decisions, large-pack streaming decisions, source-kind/materialization progress, and operation history progress.
- This triage adds a commit-boundary and validation map for the current dirty work.

Release-hardening fit:

- Keep these with the implementation commit if the project wants one auditable release-hardening changeset.
- Split them into a documentation-only commit only if the implementation is already being staged into multiple commits and the docs would otherwise become hard to match.

Recommended verification commands:

```powershell
git diff --check
```

Risks:

- Ledger docs are already modified by prior work. Do not rewrite or reorder existing entries during cleanup.
- `bugs.md` contains resolved release-risk notes. If implementation commits are split, make sure the resolved notes land with the corresponding implementation proof.

## Same-Bundle Candidates

The following files look like one coherent release-hardening bundle if hunk-level staging is not desired:

```text
src-tauri/Cargo.lock
src-tauri/Cargo.toml
src-tauri/src/backup.rs
src-tauri/src/commands.rs
src-tauri/src/diagnostics.rs
src-tauri/src/hash.rs
src-tauri/src/main.rs
src-tauri/src/manifest.rs
src-tauri/src/patch.rs
src-tauri/src/updater.rs
src/App.css
src/App.tsx
bugs.md
decisions.md
progress.md
```

Bundle name suggestion:

```text
release-hardening: materialized pack sources, streaming writes, and operation history
```

Why this bundle is coherent:

- Manifest-only source detection feeds backend target preparation.
- Target preparation feeds preview/apply source-kind gating.
- Streaming copy/hash/download helpers protect the same preview/apply/release paths.
- Operation files and UI history make the write path inspectable after completion.
- Docs already describe these as one release-readiness lane.

Downside:

- The bundle is large. `commands.rs`, `manifest.rs`, and `patch.rs` have broad changes, so review is harder.
- A later revert would be too coarse if only one sub-theme fails.

## Recommended Split If Hunk Staging Is Acceptable

### Commit 1: source-kind contract and manifest-only materialization

Files/hunks:

```text
src-tauri/Cargo.toml
src-tauri/Cargo.lock
src-tauri/src/hash.rs
src-tauri/src/manifest.rs
src-tauri/src/commands.rs
src/App.tsx
```

Scope:

- `SourceKind`
- Modrinth and CurseForge manifest-only recognition/materialization
- SHA1/SHA512 helper support
- UI source-kind warning and apply gating

Verification:

```powershell
cargo fmt --manifest-path src-tauri\Cargo.toml --check
cargo test --manifest-path src-tauri\Cargo.toml
npm run build
npm run lint
```

### Commit 2: streaming I/O and validated source writes

Files/hunks:

```text
src-tauri/src/diagnostics.rs
src-tauri/src/hash.rs
src-tauri/src/manifest.rs
src-tauri/src/patch.rs
src-tauri/src/updater.rs
```

Scope:

- Streaming update downloads and extraction.
- Streaming diagnostic attachment packaging.
- ZIP source index cache.
- Bounded preview/manifest/config reads.
- Temporary file plus SHA validation before rename.

Verification:

```powershell
cargo fmt --manifest-path src-tauri\Cargo.toml --check
cargo test --manifest-path src-tauri\Cargo.toml
npm run build
npm run lint
npm run tauri:build
```

### Commit 3: operation detail history and launcher metadata mapping

Files/hunks:

```text
src-tauri/src/backup.rs
src-tauri/src/commands.rs
src-tauri/src/main.rs
src-tauri/src/patch.rs
src/App.tsx
src/App.css
```

Scope:

- Backup operation detail schema.
- `get_backup_detail`.
- Operation history/detail UI.
- Launcher metadata source-path mapping.
- File-tree rendering reuse.

Verification:

```powershell
cargo fmt --manifest-path src-tauri\Cargo.toml --check
cargo test --manifest-path src-tauri\Cargo.toml
npm run build
npm run lint
```

### Commit 4: ledger and release notes alignment

Files:

```text
bugs.md
decisions.md
progress.md
WORKTREE_TRIAGE.md
```

Scope:

- Project ledgers and triage notes only.

Verification:

```powershell
git diff --check
```

## Files Recommended For Separate Validation

These do not necessarily need separate commits, but they need separate proof:

```text
src-tauri/src/updater.rs
```

Reason:

- Affects app self-update download/extraction trust and release delivery. Validate with release packaging and update-install smoke.

```text
src-tauri/src/patch.rs
```

Reason:

- Affects live instance writes, merge fallback, metadata replacement, managed-file tracking, and rollback-adjacent behavior.

```text
src-tauri/src/commands.rs
```

Reason:

- Largest change surface. Includes network materialization, Tauri API shape, preview/apply integration, and tests.

```text
src/App.tsx
src/App.css
```

Reason:

- Affects primary update-page usability. Needs visual smoke at desktop and narrow widths.

```text
src-tauri/src/backup.rs
```

Reason:

- Backup manifest schema extension should be checked against existing backups without `operation_files`.

## Cleanup Plan

1. Avoid broad cleanup until the current release-hardening bundle has a successful local proof.
2. Run light verification first:

```powershell
git diff --check
cargo fmt --manifest-path src-tauri\Cargo.toml --check
cargo test --manifest-path src-tauri\Cargo.toml
npm run build
npm run lint
```

3. If light verification passes, decide whether to commit as one large release-hardening bundle or hunk-stage into the four splits above.
4. Before any release tag or portable handoff, run release verification:

```powershell
npm audit --audit-level=moderate
npm run tauri:build
npm run portable:release
```

5. Do manual smoke on the four paths that automated tests do not fully prove:

```text
Complete pack update.
Modrinth manifest-only materialization.
CurseForge manifest-only missing-key and real-key behavior.
Existing backup detail and rollback.
```

## Current Triage Conclusion

This dirty work is not random. It is mostly one release-hardening lane:

```text
large-pack safety + manifest-only target handling + verified writes + inspectable operation history
```

The safest commit boundary for review quality is hunk-level split. The safest commit boundary for avoiding accidental partial staging is one release-hardening commit, followed by focused validation and, if needed, follow-up cleanup commits.

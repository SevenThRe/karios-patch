# Decisions

## 2026-05-21

- Production self-update must stay official-source-only for this release line: update indexes are accepted only from `SevenThRe/karios-patch` GitHub Release assets, and portable archive URLs must match the same official release version and asset naming convention.
- Treat update metadata as untrusted even after download: version strings must be normalized to SemVer path components, cached archive paths must be revalidated before install, and archive SHA must be checked again at staging time.
- Keep the Tauri frontend capability surface smaller than the Rust command surface. The frontend does not need direct opener plugin permissions; Rust-owned commands may open/reveal only after local validation.
- Superseded: The first progress pass used a lightweight UI-side bar. Update execution now reports real write-stage progress through Tauri events while preserving the final apply result.
- Large-pack ZIP handling must stay file-backed and streaming-oriented. Do not reintroduce whole-archive `fs::read` for scanning or source-file lookup paths.
- Keep rollback semantics state-driven: restore backed-up files, remove files that became managed only after the selected backup, and preserve user extra files.
- Treat `config/` switching as line-level three-way merge: preserve user-only lines by default, apply official-only line changes, and keep same-line competing edits as manual conflicts.
- Use `windows_subsystem = "windows"` for non-debug Windows builds so normal users see only the desktop UI, not a console host.
- Use Windows `CREATE_NO_WINDOW` for internal helper commands where the app intentionally spawns a shell/system process.
- Pivot the product shell away from a SaaS dashboard pattern and toward a desktop update workbench. The first screen should answer whether the pack can be safely updated and what user files are protected.
- Keep a small rail navigation instead of a full sidebar. Primary navigation is limited to Update, Backups, and Settings.
- Use Monaco Editor for built-in file diff because it provides a VS Code-like diff editor in React and matches the desired Cursor/VS Code interaction model.
- Treat full file diff as secondary detail. The main update screen shows a short safety summary first, then exposes the complete file diff list below.
- Do not depend on historical manifests or platform release APIs as the default model. Many packs are distributed through ad hoc ZIP files, so the default flow must work from current instance plus target pack.
- In no-baseline mode, classify uncertain local-only mods and same-path changed configs as review items. They must be visible and selectable instead of silently preserved or removed.
- Superseded: Conservative execution was disabled until review choices had a backend apply path. The no-baseline UI now executes through `apply_conservative_update`; baseline mode remains the precision path when an old official source is available.
- Execute no-baseline conservative apply by recomputing the preview plan server-side, validating all review choices before any write, then applying automatic safe actions plus explicit choices. Unsupported or stale choice keys fail before file changes.
- In no-baseline conservative apply, write target config candidates selected as `save_target_as_new` under `.packdelta/conservative-candidates/` instead of overwriting the live instance file.
- Do not use four-corner scan-frame lines in the Kairos Patch icon. The approved mark is the faceted 3D Kairos K reference: blue/teal left planes, green right planes, and dark negative-space cuts.
- Keep Patch-specific meaning through the forward-leaning K/update silhouette rather than the old standalone purple lightning mark or the interim package-box variant.
- Pin `monaco-editor` to `0.53.0` for the 0.1.2 release line because newer `0.55.1` pulls vulnerable `dompurify@3.2.7`. `@monaco-editor/react@4.7.0` supports `monaco-editor >=0.25.0 <1`, and the downgrade keeps the built-in diff editor while clearing the audit blocker.
- Ship feedback as local diagnostics plus a prefilled GitHub Issue URL first. Do not silently upload logs or instance metadata without an explicit authenticated upload design and user consent flow.
- The Kairos Patch icon should read as a K-shaped Minecraft-grass-block-inspired object, not as a literal cube. The K silhouette is the primary identity; the block palette is the material treatment.
- Update execution should report progress through Tauri events while preserving the existing request/response apply result. The UI shows a persistent operation toast and keeps final logs visible on the update page.
- Use a GitHub Issue Form template as the public feedback surface and generate a local `github-issue-body.md` companion file from the desktop app. Issue Forms are better for repository intake, while the local markdown file preserves auto-collected metadata without forcing long URL prefill payloads.
- Keep feedback diagnostics opt-in for logs and config. The default package may include the structured report and issue body, but app log upload and config snapshot/state files require explicit user toggles.
- The main update surface should be a split-pane desktop utility, not a designed dashboard/workbench page. Changes and diff are the core skeleton; summaries belong in the status bar and row metadata.

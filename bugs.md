# Bugs

## 2026-05-21

- Production release risk: the portable self-update trust model accepts any HTTPS update index and trusts the same unsigned index for both the archive URL and SHA256, so the checksum provides integrity against transfer corruption but not publisher authenticity.
- Production release risk: update cache/staging paths are built from release versions supplied by the update metadata without sanitizing them as path components, which can escape the intended update cache if malicious metadata reaches the installer path.
- Production release risk: the desktop window currently has CSP disabled and broad opener permissions, increasing the blast radius if bundled frontend code or persisted UI state is ever compromised.
- Resolved: the self-update path now accepts only the official `SevenThRe/karios-patch` GitHub Release index, requires `app_id: "kairos-patch"`, validates the portable archive URL against the matching official release asset, and keeps SHA256 validation.
- Resolved: update versions are normalized to SemVer path components, and portable install revalidates the cached archive path and SHA before staging.
- Resolved: Tauri now has an explicit CSP, frontend direct opener permissions were removed, and the custom open-folder bridge now rejects URL-like inputs and reveals files instead of opening arbitrary files directly.
- Performance/reliability risk: update downloads, portable ZIP extraction, selected diagnostic attachments, and individual ZIP source-file reads still use whole-file buffering without explicit size limits, leaving large archives or attachments able to consume excessive memory/disk.
- Release process gap: the repository currently has GitHub Issue templates but no CI workflow under `.github/workflows`, so release gates rely on local manual execution unless added elsewhere.
- Users had no durable progress indicator after clicking the protected update/rollback actions, so long-running operations looked like nothing happened.
- Backup refresh after apply/rollback could replace the meaningful completion message with generic backup-list text.
- Config auto-merge was too position-sensitive: user-only inserted lines in `config/` could force a manual conflict even when official changes touched a different line.
- Windows release builds could show an extra black console window because the Tauri binary was not marked as a Windows GUI subsystem application.
- Internal Windows command launches such as portable self-update PowerShell and `tasklist` process checks could also surface unwanted console windows.
- Resolved: `npm install @monaco-editor/react monaco-editor` reported 2 moderate npm audit findings through `monaco-editor@0.55.1 -> dompurify@3.2.7`. Pinning `monaco-editor` to `0.53.0` removes the vulnerable transitive dependency and `npm audit --audit-level=moderate` now reports 0 vulnerabilities.
- Resolved: No-baseline preview exposed local-only mods as review items, but applying those review choices was not implemented yet. The UI now calls `apply_conservative_update` with review choices, and the backend validates choices before writing.
- Resolved: The first feedback flow opened a plain GitHub Issue body and did not expose the requested structured fields, consent toggles, attachment handling, or automatic metadata contract. Feedback now uses an Issue Form template and local structured diagnostics.
- Resolved: Instance validation accepted any directory with `mods/` in baseline mode and only checked existence in no-baseline mode. Both paths now require a Minecraft game-directory marker such as `.minecraft`, `versions/`, launcher metadata, or an isolated version directory.
- Resolved: The operation toast could show `Update completed` while the indeterminate progress animation kept moving because the completion event used a one-step total.
- Resolved: No-baseline config differences under `config/**` were shown as blocking review choices, which could ask users to confirm hundreds of safe default-keep config files.

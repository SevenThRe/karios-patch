# Kairos Patch

Kairos Patch is a Windows desktop workbench for updating Minecraft modpack instances while preserving user-owned files.

It compares a current instance against a target modpack source, previews the planned file changes, protects local saves and player settings, and writes rollback data under `.packdelta`.

## What It Updates

Kairos Patch treats these paths as pack-managed by default:

- `mods/*.jar`
- `defaultconfigs/**`
- `kubejs/startup_scripts/**`
- `kubejs/server_scripts/**`
- `kubejs/client_scripts/**`
- `scripts/**`
- `libraries/**`
- `packmenu/**`

These paths are treated as user-owned or protected by default:

- `config/**`
- `resourcepacks/**`
- `shaderpacks/**`
- `saves/**`
- `screenshots/**`
- `logs/**`
- `crash-reports/**`
- `journeymap/**`
- `xaero/**`
- `options.txt`
- `optionsof.txt`
- `servers.dat`

Config files are merged conservatively when an old official source is available. Same-line local and official edits become review conflicts instead of being overwritten.

## Sources

The app accepts official pack sources as either:

- an extracted folder
- a `.zip` archive

CurseForge-style `overrides/` paths are normalized to instance-relative paths.

## Update Modes

### Baseline Mode

Provide all three inputs:

- current instance
- current baseline pack
- target pack

This is the precise mode. Kairos Patch can tell which files changed officially, which files changed locally, and which files can be safely replaced, merged, removed, or preserved.

### Conservative Mode

Provide:

- current instance
- target pack

This mode does not assume historical knowledge. It previews safe automatic actions and review items. Unknown local files are kept unless the user explicitly chooses otherwise.

## Rollback

Before writing managed changes, Kairos Patch creates a backup under:

```text
<instance>/.packdelta/backups/
```

Rollback restores backed-up files and removes files that became managed only after the selected backup. User extra files are preserved.

## Feedback Diagnostics

The Settings page includes a feedback form for bugs, suggestions, install failures, update failures, and other reports. It creates a local diagnostic package and opens the GitHub Issue Form template.

The form asks for:

- issue type, title, description, and reproduction steps
- explicit log upload consent
- explicit config upload consent
- optional screenshots or attachments
- optional contact information

The diagnostic package is written under the user's local application data directory and includes `report.json` plus `github-issue-body.md`. When the user opts in, it also includes recent app logs, a redacted config snapshot, `.packdelta/state.json`, recent conflict notes, and manually selected attachments.

Automatic metadata is generated in the report with the tool name, tool version, patch version, Windows/architecture details, Java version, error fingerprint, the recent 500-line log tail when enabled, and the redacted config snapshot when enabled.

The package is not uploaded silently. Users decide whether to attach the generated zip to the GitHub Issue.

## Development

Install dependencies:

```powershell
npm install
```

Run the web shell:

```powershell
npm run dev
```

Run the desktop app:

```powershell
npm run tauri:dev
```

Build the frontend:

```powershell
npm run build
```

Run Rust tests:

```powershell
cd src-tauri
cargo test
```

Run lint:

```powershell
npm run lint
```

## Portable Package

Debug portable package:

```powershell
npm run portable:debug
```

Release portable package:

```powershell
npm run portable:release
```

The package script writes output under `dist-portable/`.

# GitHub Update Source

Kairos Patch portable builds update from a static JSON index published as a GitHub Release asset.

Default source:

```text
https://github.com/SevenThRe/karios-patch/releases/latest/download/release-index.json
```

Recommended release assets:

```text
release-index.json
KairosPatch-v0.1.3-portable.zip
KairosPatch-v0.1.3-portable.zip.sha256.json
```

`release-index.json`:

```json
{
  "app_id": "kairos-patch",
  "latest": "0.1.3",
  "releases": [
    {
      "version": "0.1.3",
      "notes": "Fixes portable hot-update installation, supports CurseForge overrides-only ZIP imports in no-baseline mode, verifies writes before reporting completion, and refreshes the actionable update plan after apply.",
      "published_at": "2026-05-22T01:03:06Z",
      "portable": {
        "url": "https://github.com/SevenThRe/karios-patch/releases/download/v0.1.3/KairosPatch-v0.1.3-portable.zip",
        "sha256": "ce61d35b161bcf6a299b84f2b461f3fb40f01cba298a83f2d40b45bd6bebf917",
        "size": 3715331
      }
    }
  ]
}
```

The application only accepts the official `SevenThRe/karios-patch` GitHub Release update index URL. The index must use `app_id: "kairos-patch"`, the release version must be valid SemVer, and the portable archive URL must point to the matching official GitHub Release asset name.

The portable archive SHA256 is still verified before staging. This protects against transfer corruption and mismatched assets, while the official-source allowlist prevents a custom index from redirecting the updater to an arbitrary publisher.

Portable release workflow:

```powershell
npm run tauri -- build --release
npm run portable:release
```

Upload the generated zip under `dist-portable/` to GitHub Releases, update `release-index.json`, and upload the index as a release asset. The production update source must remain under `https://github.com/SevenThRe/karios-patch/releases/.../download/release-index.json`.

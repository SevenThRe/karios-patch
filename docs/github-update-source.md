# GitHub Update Source

Kairos Patch portable builds update from a static JSON index published as a GitHub Release asset.

Default source:

```text
https://github.com/SevenThRe/karios-patch/releases/latest/download/release-index.json
```

Recommended release assets:

```text
release-index.json
KairosPatch-v0.1.1-portable.zip
KairosPatch-v0.1.1-portable.zip.sha256.json
```

`release-index.json`:

```json
{
  "app_id": "kairos-patch",
  "latest": "0.1.1",
  "releases": [
    {
      "version": "0.1.1",
      "notes": "Portable update package.",
      "published_at": "2026-05-18T00:00:00Z",
      "portable": {
        "url": "https://github.com/SevenThRe/karios-patch/releases/download/v0.1.1/KairosPatch-v0.1.1-portable.zip",
        "sha256": "PUT_SHA256_HERE",
        "size": 12345678
      }
    }
  ]
}
```

The application only accepts `https://` update source URLs and verifies the portable zip SHA256 before staging it.

Portable release workflow:

```powershell
npm run tauri -- build --release
npm run portable:release
```

Upload the generated zip under `dist-portable/` to GitHub Releases, update `release-index.json`, and upload the index as a release asset. Users can paste the raw `release-index.json` asset URL into Kairos Patch settings.

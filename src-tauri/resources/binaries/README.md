# Bundled Binary Archives

Place optional per-platform FFmpeg, FFprobe, and whisper.cpp archives here.

The archives are not committed by default. The downloaded `.zip` files are
ignored so local testing does not accidentally stage third-party binary blobs.
For a Windows x64 checkout, populate the local archives with:

```sh
deno task binaries:download
```

From another platform, prepare Windows archives with:

```sh
deno task binaries:download:windows
```

Expected layout:

```text
src-tauri/resources/binaries/
  macos-aarch64/
    ffmpeg.zip
    whisper.zip
  macos-x86_64/
    ffmpeg.zip
    whisper.zip
  windows-x86_64/
    ffmpeg.zip
    whisper.zip
  linux-x86_64/
    ffmpeg.zip
    whisper.zip
```

Each archive may contain binaries at its root, in `bin/`, or one immediate
directory below the archive root with an optional `bin/` child. The app extracts
all `.zip` files for the current `os-arch` directory into app data on startup,
then resolves binaries from the extracted directory before falling back to
system `PATH` and common install locations.

Recognized executable names:

- `ffmpeg`
- `ffprobe`
- `whisper-cli`
- `main`
- `whisper`

Whisper model files are not bundled here. Keep models under the app data
`models/whisper` directory or import them through the app UI.

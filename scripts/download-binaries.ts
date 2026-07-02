type GitHubAsset = {
  name: string;
  browser_download_url: string;
  size: number;
  digest?: string;
};

type GitHubRelease = {
  tag_name: string;
  name: string;
  assets: GitHubAsset[];
};

type DownloadSpec = {
  source: string;
  repo: string;
  destinationName: string;
  selectAsset: (release: GitHubRelease) => GitHubAsset;
};

type CliOptions = {
  platform: string;
  force: boolean;
  dryRun: boolean;
};

const supportedPlatforms = new Set(["windows-x86_64"]);

const specsByPlatform: Record<string, DownloadSpec[]> = {
  "windows-x86_64": [
    {
      source: "FFmpeg",
      repo: "BtbN/FFmpeg-Builds",
      destinationName: "ffmpeg.zip",
      selectAsset: selectWindowsFfmpegAsset,
    },
    {
      source: "whisper.cpp",
      repo: "ggml-org/whisper.cpp",
      destinationName: "whisper.zip",
      selectAsset: (release) => {
        const asset = release.assets.find((asset) =>
          asset.name === "whisper-bin-x64.zip"
        );
        if (!asset) {
          throw new Error(
            "Unable to find whisper-bin-x64.zip in the latest whisper.cpp release.",
          );
        }
        return asset;
      },
    },
  ],
};

const options = parseArgs(Deno.args);

if (!supportedPlatforms.has(options.platform)) {
  throw new Error(
    [
      `Unsupported bundled binary platform: ${options.platform}`,
      "Currently supported: windows-x86_64",
      "For Windows testing, run: deno task binaries:download -- --platform windows-x86_64",
    ].join("\n"),
  );
}

const platformDirectory = new URL(
  `../src-tauri/resources/binaries/${options.platform}/`,
  import.meta.url,
);

await Deno.mkdir(platformDirectory, { recursive: true });

for (const spec of specsByPlatform[options.platform]) {
  const release = await fetchLatestRelease(spec.repo);
  const asset = spec.selectAsset(release);
  const destination = new URL(spec.destinationName, platformDirectory);

  if (!options.force && await fileExists(destination)) {
    console.log(
      `${spec.destinationName} already exists; use --force to replace it.`,
    );
    continue;
  }

  console.log(
    [
      `${spec.source}: ${release.tag_name || release.name}`,
      `  asset: ${asset.name}`,
      `  size: ${formatBytes(asset.size)}`,
      `  target: ${destination.pathname}`,
    ].join("\n"),
  );

  if (options.dryRun) {
    continue;
  }

  await downloadAsset(asset, destination);
}

function parseArgs(args: string[]): CliOptions {
  let platform = detectPlatform();
  let force = false;
  let dryRun = false;

  for (let index = 0; index < args.length; index += 1) {
    const arg = args[index];
    if (arg === "--") {
      continue;
    } else if (arg === "--help" || arg === "-h") {
      printHelpAndExit();
    } else if (arg === "--force") {
      force = true;
    } else if (arg === "--dry-run") {
      dryRun = true;
    } else if (arg === "--platform") {
      platform = args[index + 1] ?? "";
      index += 1;
    } else if (arg.startsWith("--platform=")) {
      platform = arg.slice("--platform=".length);
    } else {
      throw new Error(`Unknown argument: ${arg}`);
    }
  }

  return { platform, force, dryRun };
}

function printHelpAndExit(): never {
  console.log(
    [
      "Download bundled FFmpeg/Whisper archives for local package testing.",
      "",
      "Usage:",
      "  deno task binaries:download",
      "  deno task binaries:download -- --platform windows-x86_64",
      "",
      "Options:",
      "  --platform <target>  Target resource folder. Default: current OS/arch.",
      "  --force              Replace existing archives.",
      "  --dry-run            Resolve assets without downloading.",
    ].join("\n"),
  );
  Deno.exit(0);
}

function detectPlatform(): string {
  const os = Deno.build.os === "darwin" ? "macos" : Deno.build.os;
  return `${os}-${Deno.build.arch}`;
}

async function fetchLatestRelease(repo: string): Promise<GitHubRelease> {
  const response = await fetch(
    `https://api.github.com/repos/${repo}/releases/latest`,
    {
      headers: {
        "Accept": "application/vnd.github+json",
        "User-Agent": "metafy-desktop-binary-downloader",
      },
    },
  );
  if (!response.ok) {
    throw new Error(
      `Unable to fetch latest release for ${repo}: ${response.status}`,
    );
  }
  return await response.json() as GitHubRelease;
}

function selectWindowsFfmpegAsset(release: GitHubRelease): GitHubAsset {
  const stableAssets = release.assets
    .map((asset) => ({
      asset,
      version: windowsFfmpegStableVersion(asset.name),
    }))
    .filter((entry): entry is { asset: GitHubAsset; version: number[] } =>
      entry.version !== null
    )
    .sort((left, right) => compareVersions(right.version, left.version));

  if (stableAssets.length > 0) {
    return stableAssets[0].asset;
  }

  const masterAsset = release.assets.find((asset) =>
    asset.name === "ffmpeg-master-latest-win64-gpl.zip"
  );
  if (!masterAsset) {
    throw new Error(
      "Unable to find a Windows x64 GPL FFmpeg zip in the latest BtbN release.",
    );
  }
  return masterAsset;
}

function windowsFfmpegStableVersion(name: string): number[] | null {
  const match = name.match(/^ffmpeg-n([0-9.]+)-latest-win64-gpl-[0-9.]+\.zip$/);
  if (!match) return null;
  return match[1].split(".").map((part) => Number(part));
}

function compareVersions(left: number[], right: number[]): number {
  const length = Math.max(left.length, right.length);
  for (let index = 0; index < length; index += 1) {
    const difference = (left[index] ?? 0) - (right[index] ?? 0);
    if (difference !== 0) return difference;
  }
  return 0;
}

async function downloadAsset(asset: GitHubAsset, destination: URL) {
  const response = await fetch(asset.browser_download_url, {
    headers: { "User-Agent": "metafy-desktop-binary-downloader" },
  });
  if (!response.ok) {
    throw new Error(`Unable to download ${asset.name}: ${response.status}`);
  }

  const buffer = await response.arrayBuffer();
  await verifyDigest(asset, buffer);
  const bytes = new Uint8Array(buffer);

  const temporaryDestination = new URL(destination.href);
  temporaryDestination.pathname = `${temporaryDestination.pathname}.download`;
  await Deno.writeFile(temporaryDestination, bytes);
  await Deno.rename(temporaryDestination, destination);
  console.log(`Downloaded ${asset.name}.`);
}

async function verifyDigest(asset: GitHubAsset, buffer: ArrayBuffer) {
  if (!asset.digest?.startsWith("sha256:")) return;

  const expected = asset.digest.slice("sha256:".length).toLowerCase();
  const actualBuffer = await crypto.subtle.digest("SHA-256", buffer);
  const actual = Array.from(new Uint8Array(actualBuffer))
    .map((byte) => byte.toString(16).padStart(2, "0"))
    .join("");

  if (actual !== expected) {
    throw new Error(
      `Digest mismatch for ${asset.name}. Expected ${expected}, got ${actual}.`,
    );
  }
}

async function fileExists(path: URL): Promise<boolean> {
  try {
    await Deno.stat(path);
    return true;
  } catch (error) {
    if (error instanceof Deno.errors.NotFound) return false;
    throw error;
  }
}

function formatBytes(bytes: number): string {
  const units = ["B", "KB", "MB", "GB"];
  let value = bytes;
  let unitIndex = 0;
  while (value >= 1024 && unitIndex < units.length - 1) {
    value /= 1024;
    unitIndex += 1;
  }
  return `${value.toFixed(unitIndex === 0 ? 0 : 1)} ${units[unitIndex]}`;
}

// `bun run release [notes]`: the signed release build of the version in tauri.conf.json, and the
// two files a GitHub release needs for auto-update (ADR-0009): the installer under its asset name
// and `latest.json`. It never creates the release or pushes anything; upload the two files it
// prints to the `v<version>` release yourself.
//
//   PT_UPDATER_KEY    the updater's private key file (default ~/.tauri/productivity-tracker-updater.key).
//                     Only its path is passed on, to the Tauri CLI; the key is never printed.
//   CARGO_TARGET_DIR  where to build (default C:/dev/.cargo-target/pt-release)
//   notes             latest.json's notes (default: a link to the release page)
import { spawnSync } from "node:child_process";
import { copyFileSync, existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { join, resolve } from "node:path";

const REPO = "https://github.com/rithwik1510/productivity-tracker";

const root = resolve(import.meta.dirname, "..");
const { productName, version } = JSON.parse(
  readFileSync(join(root, "src-tauri", "tauri.conf.json"), "utf8"),
);
const key =
  process.env.PT_UPDATER_KEY ?? join(homedir(), ".tauri", "productivity-tracker-updater.key");
const targetDir = resolve(process.env.CARGO_TARGET_DIR ?? "C:/dev/.cargo-target/pt-release");
const notes = process.argv[2] ?? `${REPO}/releases/tag/v${version}`;

if (!existsSync(key)) {
  console.error(`No updater key at ${key}. Set PT_UPDATER_KEY to the key file's path.`);
  process.exit(1);
}

// Updater artifacts (the installer's .sig) are on for this build only, so a plain
// `bun run tauri build` from source still works without the key.
mkdirSync(targetDir, { recursive: true });
const config = join(targetDir, "updater-artifacts.conf.json");
writeFileSync(config, JSON.stringify({ bundle: { createUpdaterArtifacts: true } }));

const build = spawnSync("bun", ["tauri", "build", "--config", config], {
  cwd: root,
  stdio: "inherit",
  env: {
    ...process.env,
    CARGO_TARGET_DIR: targetDir,
    // The CLI reads the key from this path. The key has no password; an empty one keeps the CLI
    // from prompting for it.
    TAURI_SIGNING_PRIVATE_KEY: key,
    TAURI_SIGNING_PRIVATE_KEY_PASSWORD: "",
  },
});
if (build.status !== 0) {
  console.error(build.error ?? `tauri build failed (exit ${build.status})`);
  process.exit(1);
}

const nsis = join(targetDir, "release", "bundle", "nsis");
const built = join(nsis, `${productName}_${version}_x64-setup.exe`);
const asset = `${productName.replaceAll(" ", "")}_${version}_x64-setup.exe`;
const installer = join(nsis, asset);
copyFileSync(built, installer);

// The signature is of the file's bytes, so it holds for the renamed copy too.
const latest = join(nsis, "latest.json");
writeFileSync(
  latest,
  `${JSON.stringify(
    {
      version,
      notes,
      pub_date: new Date().toISOString(),
      platforms: {
        "windows-x86_64": {
          signature: readFileSync(`${built}.sig`, "utf8").trim(),
          url: `${REPO}/releases/download/v${version}/${asset}`,
        },
      },
    },
    null,
    2,
  )}\n`,
);

console.log(`\nUpload both to the v${version} release:\n  ${installer}\n  ${latest}`);

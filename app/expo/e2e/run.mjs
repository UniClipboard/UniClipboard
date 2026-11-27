import { execFile } from "node:child_process";
import { createHash } from "node:crypto";
import {
  copyFileSync,
  cpSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  realpathSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const args = process.argv.slice(2);
if (args.includes("--help")) {
  console.log(
    "node e2e/run.mjs --platform ios|android --device SIMULATOR_ID --app /absolute/path/to/App.app|app.apk [--suite shell|live|migration]",
  );
  process.exit(0);
}
const options = {};
for (let index = 0; index < args.length; index += 2) {
  if (
    !["--platform", "--device", "--app", "--suite"].includes(args[index]) ||
    !args[index + 1]
  )
    throw new Error("Use --help for arguments");
  options[args[index].slice(2)] = args[index + 1];
}
if (
  !["ios", "android"].includes(options.platform) ||
  !options.device ||
  !options.app
)
  throw new Error(
    "Explicit platform, simulator/emulator ID and app artifact are required",
  );
const appId = "app.uniclipboard.shell";
const suite = options.suite ?? "shell";
if (!["shell", "live", "migration"].includes(suite)) throw new Error("Unknown E2E suite");
const output = join(
  root,
  "e2e/results",
  `${new Date().toISOString().replaceAll(/[:.]/g, "-")}-${options.platform}-${process.pid}`,
);
mkdirSync(output, { recursive: true });
const receipt = {
  kind: "native-ui-e2e",
  platform: options.platform,
  device: options.device,
  appId,
  suite,
  status: "failed",
  stages: [],
};
function command(program, argv, stage, extraEnv = {}) {
  return new Promise((resolveCommand, rejectCommand) => {
    const child = execFile(program, argv, {
      cwd: root,
      encoding: "utf8",
      maxBuffer: 20 * 1024 * 1024,
      timeout: 15 * 60 * 1000,
      env: {
        ...process.env,
        MAESTRO_CLI_NO_ANALYTICS: "1",
        MAESTRO_CLI_DISABLE_UPDATE_CHECK: "1",
        ...extraEnv,
      },
    }, (error, stdout, stderr) => {
      try {
        writeFileSync(join(output, `${stage}.log`), stdout + stderr);
        receipt.stages.push({
          name: stage,
          exitCode: error ? (typeof error.code === "number" ? error.code : null) : 0,
          error: error?.message,
        });
        if (error) rejectCommand(new Error(`${stage} failed`));
        else resolveCommand(stdout.trim());
      } catch (logError) {
        rejectCommand(logError);
      }
    });
    child.stdin.on("error", () => {});
    child.stdin.end();
  });
}
async function installIOSClipboardFixture() {
  const source = join(root, "e2e/fixtures/ios-clipboard");
  const bundle = join(output, "ClipboardFixture.app");
  mkdirSync(bundle);
  copyFileSync(join(source, "Info.plist"), join(bundle, "Info.plist"));
  const sdk = await command(
    "xcrun", ["--sdk", "iphonesimulator", "--show-sdk-path"], "fixture-sdk",
  );
  const arch = process.arch === "x64" ? "x86_64" : process.arch;
  await command("xcrun", [
    "--sdk", "iphonesimulator", "swiftc", "-parse-as-library",
    "-target", `${arch}-apple-ios17.0-simulator`, "-sdk", sdk,
    join(source, "ClipboardFixture.swift"), "-o", join(bundle, "ClipboardFixture"),
  ], "build-clipboard-fixture");
  await command("codesign", ["--force", "--sign", "-", bundle], "sign-clipboard-fixture");
  receipt.clipboardFixtureSha256 = treeHash(bundle);
  await command("xcrun", ["simctl", "install", options.device, bundle], "install-clipboard-fixture");
}
const hash = (data) => createHash("sha256").update(data).digest("hex");
function treeHash(path) {
  if (!statSync(path).isDirectory()) return hash(readFileSync(path));
  function entries(dir) {
    return readdirSync(dir, { withFileTypes: true })
      .sort((a, b) => a.name.localeCompare(b.name))
      .flatMap((entry) =>
        entry.isDirectory()
          ? entries(join(dir, entry.name))
          : [join(dir, entry.name)],
      );
  }
  return hash(
    entries(path)
      .map((file) => `${relative(path, file)}:${hash(readFileSync(file))}`)
      .join("\n"),
  );
}
try {
  const artifact = realpathSync(options.app);
  receipt.artifact = {
    fileName: artifact.split("/").at(-1),
    sha256: treeHash(artifact),
  };
  receipt.packageLockSha256 = hash(
    readFileSync(join(root, "package-lock.json")),
  );
  const maestro = process.env.MAESTRO_BIN ?? "maestro";
  receipt.maestro = await command(maestro, ["--version"], "maestro-version");
  if (options.platform === "ios") {
    if (!artifact.endsWith(".app"))
      throw new Error("Expected a Simulator .app");
    const devices = JSON.parse(
      await command("xcrun", ["simctl", "list", "devices", "--json"], "device-list"),
    );
    const runtime = Object.keys(devices.devices).find((key) =>
      devices.devices[key].some((item) => item.udid === options.device && item.state === "Booted"),
    );
    if (!runtime)
      throw new Error("Select an already booted iOS Simulator");
    const actual = await command(
      "/usr/libexec/PlistBuddy",
      ["-c", "Print :CFBundleIdentifier", join(artifact, "Info.plist")],
      "artifact-id",
    );
    if (actual !== appId)
      throw new Error("Artifact is not the isolated UI shell");
    await command(
      "xcrun",
      ["simctl", "install", options.device, artifact],
      "install",
    );
    if (suite === "live") await installIOSClipboardFixture();
  } else {
    if (!artifact.endsWith(".apk") || !options.device.startsWith("emulator-"))
      throw new Error(
        "Expected an APK and an explicit Android emulator serial",
      );
    const actual = await command(
      process.env.APKANALYZER_BIN ?? "apkanalyzer",
      ["manifest", "application-id", artifact],
      "artifact-id",
    );
    if (actual !== appId)
      throw new Error("Artifact is not the isolated UI shell");
    if (
      await command(
        "adb",
        ["-s", options.device, "shell", "getprop", "ro.kernel.qemu"],
        "emulator-check",
      ) !== "1"
    )
      throw new Error("Physical devices are excluded");
    await command(
      "adb",
      ["-s", options.device, "install", "-r", "--no-streaming", artifact],
      "install",
    );
  }
  async function runMaestroFlow(name, stageName = name) {
    const flow = join(root, "e2e/flows", `${name}.yaml`);
    receipt.stages.push({
      name: `${stageName}-flow`,
      sha256: hash(readFileSync(flow)),
    });
    await command(
      maestro,
      [
        "--device",
        options.device,
        "test",
        flow,
        "--format",
        "junit",
        "--output",
        join(output, `${stageName}.xml`),
        "--test-output-dir",
        join(output, stageName),
      ],
      stageName,
    );
  }

  async function querySQLiteCount(dbPath, sql, stageName) {
    const out = await command("sqlite3", ["-readonly", dbPath, sql], stageName);
    return Number.parseInt(out.trim(), 10);
  }

  if (suite === "migration") {
    const generator = join(root, "e2e/fixtures/legacy-mobile/generate.py");
    receipt.fixtureGeneratorSha256 = hash(readFileSync(generator));
    const fixtureDir = join(output, "legacy-fixtures");
    const manifest = JSON.parse(
      await command("python3", [generator, "--out", fixtureDir], "generate-legacy-fixtures"),
    );
    receipt.legacyFixtureManifest = manifest;

    if (options.platform === "ios") {
      // Scenario A: iOS App Group layout (Databases/uniclipboard.db + extensionless payloads/).
      const agTarget = join(output, "ios-app-group-target");
      mkdirSync(agTarget, { recursive: true });
      await command("xcrun", ["simctl", "terminate", options.device, appId], "ios-ag-terminate").catch(() => {});
      await command("xcrun", ["simctl", "launch", options.device, appId], "ios-ag-launch", {
        SIMCTL_CHILD_UNIC_MOBILE_DATA_DIR: agTarget,
        SIMCTL_CHILD_UNIC_MOBILE_LEGACY_DB: manifest.ios_app_group.db_path,
        SIMCTL_CHILD_UNIC_MOBILE_LEGACY_ATTACHMENTS: manifest.ios_app_group.payloads_dir,
      });
      await runMaestroFlow("migration-done-enter", "ios-app-group-done-enter");
      const agCount = await querySQLiteCount(
        join(agTarget, "unic.db"),
        "SELECT count(*) FROM clip_items;",
        "ios-ag-verify-count",
      );
      if (agCount !== manifest.ios_app_group.active_rows) {
        throw new Error(`iOS App Group migrated count=${agCount}, expected ${manifest.ios_app_group.active_rows}`);
      }
      const agSourceHash = hash(readFileSync(manifest.ios_app_group.db_path));
      if (agSourceHash !== manifest.ios_app_group.db_sha256) {
        throw new Error("iOS App Group source uniclipboard.db was modified");
      }
      const agMarker = JSON.parse(readFileSync(join(agTarget, "mobile-setup.json"), "utf8"));
      if (!agMarker.complete || agMarker.source_kind !== "mobile_sqlite") {
        throw new Error(`Unexpected iOS App Group setup marker: ${JSON.stringify(agMarker)}`);
      }

      // Scenario B: Zero-override iOS Sandbox default path (Documents/SQLite + Documents/clipboards + Library/Caches/temp_files).
      await command("xcrun", ["simctl", "terminate", options.device, appId], "ios-sb-terminate").catch(() => {});
      await command("xcrun", ["simctl", "uninstall", options.device, appId], "ios-sb-uninstall").catch(() => {});
      await command("xcrun", ["simctl", "install", options.device, artifact], "ios-sb-reinstall");
      const container = await command("xcrun", ["simctl", "get_app_container", options.device, appId, "data"], "ios-sb-container");
      const groupContainer = await command(
        "xcrun",
        ["simctl", "get_app_container", options.device, appId, `group.${appId}.incoming-share`],
        "ios-sb-group-container",
      ).catch(() => container);
      const sbTarget = join(groupContainer, "Library/Application Support/unic-v2");
      rmSync(sbTarget, { recursive: true, force: true });
      cpSync(join(fixtureDir, "ios-sandbox/Documents/SQLite"), join(container, "Documents/SQLite"), { recursive: true });
      cpSync(join(fixtureDir, "ios-sandbox/Documents/clipboards"), join(container, "Documents/clipboards"), { recursive: true });
      mkdirSync(join(container, "Library/Caches"), { recursive: true });
      cpSync(join(fixtureDir, "ios-sandbox/Library/Caches/temp_files"), join(container, "Library/Caches/temp_files"), { recursive: true });

      await command("xcrun", ["simctl", "launch", options.device, appId], "ios-sb-launch");
      await runMaestroFlow("migration-wait-partial", "ios-sb-wait-partial");

      const sbPartialCount = await querySQLiteCount(
        join(sbTarget, "unic.db"),
        "SELECT count(*) FROM clip_items;",
        "ios-sb-verify-partial-count",
      );
      if (sbPartialCount !== manifest.ios_sandbox.initial_recoverable) {
        throw new Error(`iOS sandbox partial count=${sbPartialCount}, expected ${manifest.ios_sandbox.initial_recoverable}`);
      }
      const sbSourceHash = hash(readFileSync(join(container, "Documents/SQLite/uniclipboard.db")));
      if (sbSourceHash !== manifest.ios_sandbox.db_sha256) {
        throw new Error("iOS sandbox source uniclipboard.db was modified");
      }

      // Restore missing attachment and verify retry -> complete -> enter history.
      const repairDir = join(container, "Documents/clipboards/history/File-sb-missing-3");
      mkdirSync(repairDir, { recursive: true });
      copyFileSync(join(fixtureDir, "ios-sandbox/repair/missing.txt"), join(repairDir, "missing.txt"));
      await runMaestroFlow("migration-partial-repair", "ios-sb-partial-repair");

      const sbFinalCount = await querySQLiteCount(
        join(sbTarget, "unic.db"),
        "SELECT count(*) FROM clip_items;",
        "ios-sb-verify-final-count",
      );
      if (sbFinalCount !== manifest.ios_sandbox.active_rows) {
        throw new Error(`iOS sandbox final count=${sbFinalCount}, expected ${manifest.ios_sandbox.active_rows}`);
      }
      const sbMarker = JSON.parse(readFileSync(join(sbTarget, "mobile-setup.json"), "utf8"));
      if (!sbMarker.complete || sbMarker.source_kind !== "mobile_sqlite") {
        throw new Error(`Unexpected iOS sandbox setup marker: ${JSON.stringify(sbMarker)}`);
      }

      // Cold restart must bypass Setup and preserve migrated history.
      await command("xcrun", ["simctl", "terminate", options.device, appId], "ios-sb-stop-for-restart");
      await command("xcrun", ["simctl", "launch", options.device, appId], "ios-sb-relaunch");
      await runMaestroFlow("migration-restart-bypass", "ios-sb-restart-bypass");
    } else {
      const shell = ["-s", options.device, "shell"];
      const cleared = await command("adb", [...shell, "pm", "clear", appId], "android-mig-clear");
      if (cleared !== "Success") throw new Error("Android Emulator could not clear the test profile");

      let debuggable = false;
      try {
        await command("adb", [...shell, "run-as", appId, "true"], "android-check-debuggable");
        debuggable = true;
      } catch {
        debuggable = false;
      }
      receipt.androidStagingMode = debuggable ? "run-as-sandbox" : "external-files-intent";

      const remoteExtBase = `/sdcard/Android/data/${appId}/files/legacy-migration-fixture`;
      const androidFixtureDir = join(fixtureDir, "android");
      let launchArgs;
      let sourceDBHashCmd;

      if (debuggable) {
        const tarPath = join(output, "android-legacy.tar");
        await command("tar", ["-C", androidFixtureDir, "-cf", tarPath, "SQLite", "clipboards", "temp_files"], "android-pack-fixture", {
          COPYFILE_DISABLE: "1",
        });
        await command("adb", ["-s", options.device, "push", tarPath, "/data/local/tmp/unic_e2e_legacy.tar"], "android-push-fixture");
        await command(
          "adb",
          [...shell, `run-as ${appId} sh -c 'mkdir -p files cache && tar -xf /data/local/tmp/unic_e2e_legacy.tar -C files && mv files/temp_files cache/temp_files'`],
          "android-extract-fixture",
        );
        await command("adb", [...shell, "rm", "-f", "/data/local/tmp/unic_e2e_legacy.tar"], "android-clean-tmp-tar");
        launchArgs = [...shell, "am", "start", "-W", "-n", `${appId}/.MainActivity`];
        sourceDBHashCmd = [...shell, `run-as ${appId} sha256sum files/SQLite/uniclipboard.db`];
      } else {
        // Initialize the app-private external files dir under the app's own UID before staging fixtures.
        const initLaunch = await command("adb", [...shell, "am", "start", "-W", "-n", `${appId}/.MainActivity`], "android-init-ext-dir");
        if (!/^Status: ok$/m.test(initLaunch)) throw new Error("Android Emulator could not initialize app external dir");
        for (let i = 0; i < 20; i++) {
          try {
            await command("adb", [...shell, `test -d /sdcard/Android/data/${appId}/files`], `android-wait-ext-dir-${i}`);
            break;
          } catch {
            await new Promise((r) => setTimeout(r, 500));
          }
        }
        await command("adb", [...shell, "am", "force-stop", appId], "android-stop-after-ext-init");

        const tarPath = join(output, "android-legacy.tar");
        await command("tar", ["-C", androidFixtureDir, "-cf", tarPath, "SQLite", "clipboards", "temp_files"], "android-pack-fixture", {
          COPYFILE_DISABLE: "1",
        });
        await command("adb", [...shell, "rm", "-rf", remoteExtBase], "android-clean-ext-dir").catch(() => {});
        await command("adb", [...shell, "mkdir", "-p", remoteExtBase], "android-mkdir-ext-dir");
        await command("adb", ["-s", options.device, "push", tarPath, "/data/local/tmp/unic_e2e_legacy.tar"], "android-push-fixture");
        await command(
          "adb",
          [...shell, `tar --no-same-owner -xf /data/local/tmp/unic_e2e_legacy.tar -C ${remoteExtBase} && rm -f /data/local/tmp/unic_e2e_legacy.tar`],
          "android-extract-ext-fixture",
        );
        launchArgs = [
          ...shell,
          "am", "start", "-W", "-n", `${appId}/.MainActivity`,
          "--es", "UNIC_MOBILE_LEGACY_DB", `${remoteExtBase}/SQLite/uniclipboard.db`,
          "--es", "UNIC_MOBILE_LEGACY_ATTACHMENTS", `${remoteExtBase}/clipboards:${remoteExtBase}/temp_files`,
        ];
        sourceDBHashCmd = [...shell, "sha256sum", `${remoteExtBase}/SQLite/uniclipboard.db`];
      }

      try {
        const launch = await command("adb", launchArgs, "android-mig-launch");
        if (!/^Status: ok$/m.test(launch)) throw new Error("Android Emulator could not launch migration test");

        await runMaestroFlow("migration-wait-partial", "android-wait-partial");

        const dbHashOut = await command("adb", sourceDBHashCmd, "android-check-source-sha");
        const observedHash = dbHashOut.trim().split(/\s+/)[0];
        if (observedHash !== manifest.android.db_sha256) {
          throw new Error(`Android source uniclipboard.db modified: got ${observedHash}, want ${manifest.android.db_sha256}`);
        }

        // Restore missing attachment and tap retry -> complete -> enter history.
        const repairSrc = join(androidFixtureDir, "repair/deferred.txt");
        if (debuggable) {
          await command("adb", ["-s", options.device, "push", repairSrc, "/data/local/tmp/unic_e2e_deferred.txt"], "android-push-repair");
          await command(
            "adb",
            [...shell, `run-as ${appId} sh -c 'mkdir -p files/clipboards/history/File-android-missing-7 && cp /data/local/tmp/unic_e2e_deferred.txt files/clipboards/history/File-android-missing-7/deferred.txt'`],
            "android-stage-repair",
          );
          await command("adb", [...shell, "rm", "-f", "/data/local/tmp/unic_e2e_deferred.txt"], "android-clean-repair-tmp");
        } else {
          await command("adb", ["-s", options.device, "push", repairSrc, "/data/local/tmp/unic_e2e_deferred.txt"], "android-push-repair");
          await command(
            "adb",
            [...shell, `mkdir -p ${remoteExtBase}/clipboards/history/File-android-missing-7 && cp /data/local/tmp/unic_e2e_deferred.txt ${remoteExtBase}/clipboards/history/File-android-missing-7/deferred.txt && rm -f /data/local/tmp/unic_e2e_deferred.txt`],
            "android-stage-ext-repair",
          );
        }

        await runMaestroFlow("migration-partial-repair", "android-partial-repair");

        if (debuggable) {
          const readbackDB = join(output, "android-unic-readback.db");
          const dbRaw = await new Promise((res, rej) => {
            execFile("adb", [...shell, `run-as ${appId} cat files/unic-v2/unic.db`], { encoding: "buffer", maxBuffer: 50 * 1024 * 1024 }, (err, out) => {
              if (err) rej(err);
              else res(out);
            });
          });
          writeFileSync(readbackDB, dbRaw);
          const count = await querySQLiteCount(readbackDB, "SELECT count(*) FROM clip_items;", "android-verify-final-count");
          if (count !== manifest.android.active_rows) {
            throw new Error(`Android final count=${count}, expected ${manifest.android.active_rows}`);
          }
        }

        // Force-stop and cold relaunch with NO extras -> Setup must be bypassed and history preserved.
        await command("adb", [...shell, "am", "force-stop", appId], "android-mig-stop");
        const relaunch = await command("adb", [...shell, "am", "start", "-W", "-n", `${appId}/.MainActivity`], "android-mig-relaunch");
        if (!/^Status: ok$/m.test(relaunch)) throw new Error("Android Emulator could not relaunch after migration");
        await runMaestroFlow("migration-restart-bypass", "android-restart-bypass");
      } finally {
        if (!debuggable) {
          await command("adb", [...shell, "rm", "-rf", remoteExtBase], "android-cleanup-ext-dir").catch(() => {});
        }
      }
    }
  } else {
    const flows = suite === "live" ? ["live-setup", "live-history", "live-devices-settings"] : ["shell", "search", "selection"];
    for (const name of flows) {
      if (options.platform === "ios" && name === "live-setup") {
        const groupContainer = await command(
          "xcrun",
          ["simctl", "get_app_container", options.device, appId, `group.${appId}.incoming-share`],
          "ios-live-group-container",
        ).catch(() => "");
        if (groupContainer) {
          rmSync(join(groupContainer, "Library/Application Support/unic-v2"), {
            recursive: true,
            force: true,
          });
          rmSync(join(groupContainer, "keyboard-bridge"), {
            recursive: true,
            force: true,
          });
        }
      }
      if (options.platform === "android" && (name === "live-setup" || name === "live-devices-settings")) {
        const shell = ["-s", options.device, "shell"];
        if (name === "live-setup") {
          const cleared = await command("adb", [...shell, "pm", "clear", appId], "clear-profile");
          if (cleared !== "Success") throw new Error("Android Emulator could not clear the test profile");
        } else {
          await command("adb", [...shell, "am", "force-stop", appId], "stop-app");
        }
        const launch = await command("adb", [...shell, "am", "start", "-W", "-n", appId + "/.MainActivity"], name + "-launch");
        if (!/^Status: ok$/m.test(launch)) throw new Error("Android Emulator could not start the test app");
      }
      if (name === "live-history" && options.platform === "android") {
        await command("adb", ["-s", options.device, "shell", `am start -W -a android.intent.action.SEND -t text/plain --es android.intent.extra.TEXT 'Unic Mobile E2E' -n ${appId}/.MainActivity`], "seed-share-intent");
      }
      await runMaestroFlow(name);
    }
  }
  receipt.status = "passed";
} catch (error) {
  receipt.error = error.message;
  process.exitCode = 1;
} finally {
  receipt.finishedAt = new Date().toISOString();
  writeFileSync(
    join(output, "receipt.json"),
    JSON.stringify(receipt, null, 2) + "\n",
  );
  console.log(`${receipt.status}: ${relative(root, output)}/receipt.json`);
}

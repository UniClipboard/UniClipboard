import { execFile } from "node:child_process";
import { createHash, randomUUID } from "node:crypto";
import { once } from "node:events";
import {
  mkdirSync,
  readFileSync,
  readdirSync,
  realpathSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { createServer } from "node:http";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const args = process.argv.slice(2);
if (args.includes("--help")) {
  console.log(
    "node e2e/run.mjs --platform ios|android --device SIMULATOR_ID --app /absolute/path/to/App.app|app.apk [--suite shell|live]",
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
if (!["shell", "live"].includes(suite)) throw new Error("Unknown E2E suite");
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
function command(program, argv, stage, input) {
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
    child.stdin.end(input);
  });
}
async function seedIOSClipboard(runtimeMajor) {
  const fixture = "Unic Mobile E2E";
  let clipboardText;
  if (runtimeMajor >= 27) {
    const pasteboard = ["devicectl", "device", "pasteboard"];
    const target = ["--device", options.device, "--timeout", "45", "--quiet"];
    await command("xcrun", [...pasteboard, "copy", ...target], "seed-clipboard", fixture);
    clipboardText = await command("xcrun", [...pasteboard, "paste", ...target], "verify-clipboard");
  } else {
    await command("xcrun", ["simctl", "pbcopy", options.device], "seed-clipboard", fixture);
    clipboardText = await command("xcrun", ["simctl", "pbpaste", options.device], "verify-clipboard");
  }
  if (clipboardText !== fixture) throw new Error("iOS Simulator pasteboard does not contain the fixture");
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
  let iosRuntimeMajor;
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
    iosRuntimeMajor = Number(runtime.match(/\.iOS-(\d+)/)?.[1]);
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
  const flows = suite === "live" ? ["live-setup", "live-history", "live-devices-settings"] : ["shell", "search", "selection"];
  for (const name of flows) {
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
    let pasteboardServer;
    const parameters = [];
    if (name === "live-history" && options.platform === "ios") {
      const fixturePath = "/" + randomUUID();
      pasteboardServer = createServer(async (request, response) => {
        if (request.method !== "POST" || request.url !== fixturePath) {
          response.writeHead(404).end();
          return;
        }
        try {
          await seedIOSClipboard(iosRuntimeMajor);
          response.writeHead(204).end();
        } catch (error) {
          response.writeHead(500).end(error.message);
        }
      });
      const listening = once(pasteboardServer, "listening");
      pasteboardServer.listen(0, "127.0.0.1");
      await listening;
      parameters.push("--env", `PASTEBOARD_FIXTURE_URL=http://127.0.0.1:${pasteboardServer.address().port}${fixturePath}`);
    } else if (name === "live-history") {
      await command("adb", ["-s", options.device, "shell", `am start -W -a android.intent.action.SEND -t text/plain --es android.intent.extra.TEXT 'Unic Mobile E2E' -n ${appId}/.MainActivity`], "seed-share-intent");
    }
    const flow = join(root, "e2e/flows", `${name}.yaml`);
    receipt.stages.push({
      name: `${name}-flow`,
      sha256: hash(readFileSync(flow)),
    });
    try {
      await command(
        maestro,
        [
          "--device",
          options.device,
          "test",
          flow,
          ...parameters,
          "--format",
          "junit",
          "--output",
          join(output, `${name}.xml`),
          "--test-output-dir",
          join(output, name),
        ],
        name,
      );
    } finally {
      pasteboardServer?.close();
      pasteboardServer?.closeAllConnections();
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

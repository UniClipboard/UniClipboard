import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import {
  mkdirSync,
  readFileSync,
  readdirSync,
  realpathSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const args = process.argv.slice(2);
if (args.includes("--help")) {
  console.log(
    "node e2e/run.mjs --platform ios|android --device SIMULATOR_ID --app /absolute/path/to/App.app|app.apk",
  );
  process.exit(0);
}
const options = {};
for (let index = 0; index < args.length; index += 2) {
  if (
    !["--platform", "--device", "--app"].includes(args[index]) ||
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
const output = join(
  root,
  "e2e/results",
  new Date().toISOString().replaceAll(/[:.]/g, "-"),
);
mkdirSync(output, { recursive: true });
const receipt = {
  kind: "native-ui-e2e",
  platform: options.platform,
  device: options.device,
  appId,
  status: "failed",
  stages: [],
};
function command(program, argv, stage) {
  const result = spawnSync(program, argv, {
    cwd: root,
    encoding: "utf8",
    maxBuffer: 20 * 1024 * 1024,
    timeout: 15 * 60 * 1000,
    env: {
      ...process.env,
      MAESTRO_CLI_NO_ANALYTICS: "1",
      MAESTRO_CLI_DISABLE_UPDATE_CHECK: "1",
    },
  });
  writeFileSync(
    join(output, `${stage}.log`),
    `${result.stdout ?? ""}${result.stderr ?? ""}`,
  );
  receipt.stages.push({
    name: stage,
    exitCode: result.status,
    error: result.error?.message,
  });
  if (result.status !== 0) throw new Error(`${stage} failed`);
  return result.stdout.trim();
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
  receipt.maestro = command(maestro, ["--version"], "maestro-version");
  if (options.platform === "ios") {
    if (!artifact.endsWith(".app"))
      throw new Error("Expected a Simulator .app");
    const devices = JSON.parse(
      command("xcrun", ["simctl", "list", "devices", "--json"], "device-list"),
    );
    const device = Object.values(devices.devices)
      .flat()
      .find((item) => item.udid === options.device);
    if (!device || device.state !== "Booted")
      throw new Error("Select an already booted iOS Simulator");
    const actual = command(
      "/usr/libexec/PlistBuddy",
      ["-c", "Print :CFBundleIdentifier", join(artifact, "Info.plist")],
      "artifact-id",
    );
    if (actual !== appId)
      throw new Error("Artifact is not the isolated UI shell");
    command(
      "xcrun",
      ["simctl", "install", options.device, artifact],
      "install",
    );
  } else {
    if (!artifact.endsWith(".apk") || !options.device.startsWith("emulator-"))
      throw new Error(
        "Expected an APK and an explicit Android emulator serial",
      );
    const actual = command(
      process.env.APKANALYZER_BIN ?? "apkanalyzer",
      ["manifest", "application-id", artifact],
      "artifact-id",
    );
    if (actual !== appId)
      throw new Error("Artifact is not the isolated UI shell");
    if (
      command(
        "adb",
        ["-s", options.device, "shell", "getprop", "ro.kernel.qemu"],
        "emulator-check",
      ) !== "1"
    )
      throw new Error("Physical devices are excluded");
    command(
      "adb",
      ["-s", options.device, "install", "-r", artifact],
      "install",
    );
  }
  for (const name of ["shell", "search", "selection"]) {
    const flow = join(root, "e2e/flows", `${name}.yaml`);
    receipt.stages.push({
      name: `${name}-flow`,
      sha256: hash(readFileSync(flow)),
    });
    command(
      maestro,
      [
        "--device",
        options.device,
        "test",
        flow,
        "--format",
        "junit",
        "--output",
        join(output, `${name}.xml`),
        "--test-output-dir",
        join(output, name),
      ],
      name,
    );
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

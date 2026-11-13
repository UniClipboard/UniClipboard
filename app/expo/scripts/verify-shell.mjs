import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import { readFileSync, writeFileSync, mkdirSync, readdirSync } from "node:fs";
import { dirname, relative, resolve, join } from "node:path";
import { fileURLToPath } from "node:url";
import ts from "typescript";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
process.chdir(root);
const output = resolve("dist/verification");
mkdirSync(output, { recursive: true });
const report = {
  kind: "native-bundle-acceptance",
  startedAt: new Date().toISOString(),
  node: process.version,
  upstream: (({ repository, commit }) => ({ repository, commit }))(
    JSON.parse(readFileSync("migration-source.json")),
  ),
  checks: [],
  nativeUI: "NOT RUN",
};
const hash = (bytes) => createHash("sha256").update(bytes).digest("hex");
function files(directory) {
  return readdirSync(directory, { withFileTypes: true })
    .sort((a, b) => a.name.localeCompare(b.name))
    .flatMap((entry) => {
      if (
        ([
          "node_modules",
          "dist",
          ".expo",
          "ios",
          "android",
          "results",
        ].includes(entry.name) &&
          directory === root) ||
        (directory === join(root, "e2e") && entry.name === "results")
      )
        return [];
      const path = join(directory, entry.name);
      return entry.isDirectory() ? files(path) : [path];
    });
}
function run(name, args) {
  const result = spawnSync(process.execPath, args, {
    cwd: root,
    encoding: "utf8",
    env: { ...process.env, CI: "1", EXPO_NO_TELEMETRY: "1" },
  });
  writeFileSync(
    join(output, `${name}.log`),
    `${result.stdout ?? ""}${result.stderr ?? ""}`,
  );
  report.checks.push({
    name,
    exitCode: result.status,
    error: result.error?.message,
  });
  if (result.status !== 0)
    throw new Error(`${name} failed; see dist/verification/${name}.log`);
}
const forbidden =
  /^(?:uc-engine|android-util|app-group-store|clipboard-overlay|foreground-service|native-timer|shortcut|shizuku-clipboard|sms-verification-code|document-exporter|qr-scanner|axios|posthog-react-native|expo-(?:sqlite|clipboard|sharing|camera|file-system|notifications))(?:\/|$)|^@\/(?:features|platform|support|app)\//;
try {
  const inputs = files(root).filter(
    (file) => !file.endsWith("migration-source.json"),
  );
  report.sourceTreeSha256 = hash(
    inputs
      .map((file) => `${relative(root, file)}:${hash(readFileSync(file))}`)
      .join("\n"),
  );
  const sourceFiles = inputs.filter(
    (file) => file.startsWith(join(root, "src")) && /\.tsx?$/.test(file),
  );
  for (const file of sourceFiles) {
    const source = ts.createSourceFile(
      file,
      readFileSync(file, "utf8"),
      ts.ScriptTarget.Latest,
      true,
    );
    function visit(node) {
      let specifier;
      if (
        (ts.isImportDeclaration(node) || ts.isExportDeclaration(node)) &&
        node.moduleSpecifier
      )
        specifier = node.moduleSpecifier.text;
      if (
        ts.isCallExpression(node) &&
        ["require", "import"].includes(node.expression.getText(source)) &&
        node.arguments[0] &&
        ts.isStringLiteral(node.arguments[0])
      )
        specifier = node.arguments[0].text;
      if (specifier && forbidden.test(specifier))
        throw new Error(
          `Integration import in ${relative(root, file)}: ${specifier}`,
        );
      if (
        ts.isCallExpression(node) &&
        /^(fetch|.*\.openURL|.*\.share|.*\.setStringAsync)$/.test(
          node.expression.getText(source),
        )
      )
        throw new Error(`System/network operation in ${relative(root, file)}`);
      ts.forEachChild(node, visit);
    }
    visit(source);
  }
  report.checks.push({
    name: "source-boundary",
    files: sourceFiles.length,
    exitCode: 0,
  });
  const pkg = JSON.parse(readFileSync("package.json"));
  if (Object.keys(pkg.dependencies).some((name) => forbidden.test(name)))
    throw new Error("Integration package in dependencies");
  run("types", ["node_modules/typescript/bin/tsc", "--noEmit"]);
  run("search-tab-patch", [
    "scripts/patch-react-native-screens-search-tab.cjs",
  ]);
  run("tab-animation-patch", [
    "scripts/patch-react-native-screens-tab-bar-animation.cjs",
  ]);
  run("prebuild", [
    "node_modules/expo/bin/cli",
    "prebuild",
    "--no-install",
    "--platform",
    "all",
  ]);
  for (const platform of ["ios", "android"]) {
    run(`bundle-${platform}`, [
      "node_modules/expo/bin/cli",
      "export",
      "--platform",
      platform,
      "--output-dir",
      `dist/${platform}`,
      "--source-maps",
      "--max-workers",
      "2",
    ]);
    const bundleFiles = files(resolve(`dist/${platform}`));
    const maps = bundleFiles.filter((file) => file.endsWith(".map"));
    if (!maps.length || !bundleFiles.some((file) => file.endsWith(".hbc")))
      throw new Error(`Missing ${platform} bundle/map`);
    const sources = maps.flatMap(
      (file) => JSON.parse(readFileSync(file)).sources,
    );
    if (
      sources.some((file) =>
        /(?:^|\/)src\/(?:features|platform|support|app)\//.test(file),
      )
    )
      throw new Error(`Legacy runtime in ${platform} bundle`);
    const checksums =
      bundleFiles
        .map((file) => `${hash(readFileSync(file))}  ${relative(root, file)}`)
        .join("\n") + "\n";
    writeFileSync(join(output, `${platform}.sha256`), checksums);
    report.checks.push({
      name: `bundle-boundary-${platform}`,
      sourceModules: sources.length,
      artifacts: bundleFiles.length,
      exitCode: 0,
    });
  }
  report.status = "passed";
} catch (error) {
  report.status = "failed";
  report.error = error.message;
  process.exitCode = 1;
} finally {
  report.finishedAt = new Date().toISOString();
  writeFileSync(
    join(output, "report.json"),
    JSON.stringify(report, null, 2) + "\n",
  );
  console.log(`${report.status}: dist/verification/report.json`);
}

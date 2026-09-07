import { execFileSync } from "node:child_process";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { join, resolve } from "node:path";

export const defaultContainer = "iCloud.com.alkinum.aipass";
const bundleId = "com.alkinum.aipass";

export function cloudEntitlements(profile, team, container = defaultContainer, now = Date.now()) {
  const ent = profile.Entitlements ?? {};
  const allows = (value, required) => Array.isArray(value) ? value.includes(required) : value === required;
  const require = (condition, message) => { if (!condition) throw new Error(message); };
  require(team && profile.TeamIdentifier?.includes(team), "CloudKit profile does not match APPLE_TEAM_ID");
  require(profile.Platform?.includes("OSX"), "CloudKit requires a macOS provisioning profile");
  require(profile.ProvisionsAllDevices === true, "CloudKit release requires a Developer ID provisioning profile");
  require(Date.parse(profile.ExpirationDate) > now, "CloudKit provisioning profile expired");
  const applicationId = ent["com.apple.application-identifier"];
  require(applicationId === `${team}.${bundleId}`, "CloudKit profile application identifier does not match AIPass");
  require(allows(ent["com.apple.developer.icloud-container-identifiers"], container), "CloudKit container is missing from the profile");
  require(allows(ent["com.apple.developer.icloud-services"], "CloudKit"), "CloudKit service is missing from the profile");
  require(allows(ent["com.apple.developer.icloud-container-environment"], "Production"), "CloudKit profile does not allow Production");
  require(ent["com.apple.developer.aps-environment"] === "production", "CloudKit profile does not allow production push notifications");
  return {
    "com.apple.application-identifier": applicationId,
    "com.apple.developer.team-identifier": team,
    "com.apple.developer.icloud-container-identifiers": [container],
    "com.apple.developer.icloud-services": ["CloudKit"],
    "com.apple.developer.icloud-container-environment": "Production",
    "com.apple.developer.aps-environment": "production",
    "com.apple.security.cs.allow-jit": true,
    "com.apple.security.cs.allow-unsigned-executable-memory": true
  };
}

function plistJson(bytes) {
  return JSON.parse(execFileSync("/usr/bin/plutil", ["-convert", "json", "-o", "-", "--", "-"], { input: bytes, stdio: ["pipe", "pipe", "pipe"] }));
}

export function parseProfile(bytes) {
  // Provisioning profiles contain NSDate and certificate Data values, which
  // plutil cannot convert to JSON. Decode only the fields used by the gate.
  return JSON.parse(execFileSync("/usr/bin/python3", ["-c", [
    "import sys, plistlib, json",
    "profile = plistlib.loads(sys.stdin.buffer.read())",
    "keys = ['Platform', 'TeamIdentifier', 'ProvisionsAllDevices', 'ExpirationDate', 'Entitlements']",
    "value = {key: profile[key] for key in keys if key in profile}",
    "if 'ExpirationDate' in value: value['ExpirationDate'] = value['ExpirationDate'].isoformat() + 'Z'",
    "print(json.dumps(value))"
  ].join("\n")], { input: bytes, stdio: ["pipe", "pipe", "pipe"] }));
}

export async function prepareCloudKitProfile(outputDir, env = process.env) {
  const source = env.APPLE_PROVISIONING_PROFILE;
  const encoded = env.APPLE_PROVISIONING_PROFILE_BASE64;
  if (!source && !encoded) throw new Error("Set APPLE_PROVISIONING_PROFILE or APPLE_PROVISIONING_PROFILE_BASE64 to the AIPass Developer ID CloudKit profile");
  const container = env.AIPASS_CLOUDKIT_CONTAINER || defaultContainer;
  const directory = resolve(outputDir);
  await mkdir(directory, { recursive: true, mode: 0o700 });
  const profilePath = join(directory, "embedded.provisionprofile");
  await writeFile(profilePath, source ? await readFile(source) : Buffer.from(encoded, "base64"), { mode: 0o600 });
  const profile = parseProfile(execFileSync("/usr/bin/security", ["cms", "-D", "-i", profilePath], { stdio: ["ignore", "pipe", "pipe"] }));
  const entitlements = cloudEntitlements(profile, env.APPLE_TEAM_ID, container);
  const entitlementsPath = join(directory, "Entitlements.plist");
  await writeFile(entitlementsPath, JSON.stringify(entitlements), { mode: 0o600 });
  execFileSync("/usr/bin/plutil", ["-convert", "xml1", entitlementsPath]);
  const infoPath = join(directory, "Info.plist");
  await writeFile(infoPath, JSON.stringify({ AIPassCloudKitContainer: container }));
  execFileSync("/usr/bin/plutil", ["-convert", "xml1", infoPath]);
  return { entitlements: entitlementsPath, infoPlist: infoPath, files: { "embedded.provisionprofile": profilePath } };
}

export async function verifyCloudKitBundle(appPath, team) {
  const profilePath = join(appPath, "Contents", "embedded.provisionprofile");
  const profile = parseProfile(execFileSync("/usr/bin/security", ["cms", "-D", "-i", profilePath], { stdio: ["ignore", "pipe", "pipe"] }));
  const info = plistJson(await readFile(join(appPath, "Contents", "Info.plist")));
  const expected = cloudEntitlements(profile, team, info.AIPassCloudKitContainer);
  const actual = plistJson(execFileSync("/usr/bin/codesign", ["-d", "--entitlements", ":-", appPath], { stdio: ["ignore", "pipe", "pipe"] }));
  for (const [key, value] of Object.entries(expected)) {
    if (JSON.stringify(actual[key]) !== JSON.stringify(value)) throw new Error(`Signed CloudKit entitlement mismatch: ${key}`);
  }
}

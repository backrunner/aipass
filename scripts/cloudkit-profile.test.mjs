import { test } from "node:test";
import assert from "node:assert/strict";
import { cloudEntitlements, defaultContainer, parseProfile } from "./cloudkit-profile.mjs";

function profile() {
  return {
    Platform: ["OSX"], TeamIdentifier: ["TESTTEAM00"], ProvisionsAllDevices: true,
    ExpirationDate: "2099-01-01T00:00:00Z",
    Entitlements: {
      "com.apple.application-identifier": "TESTTEAM00.com.alkinum.aipass",
      "com.apple.developer.icloud-container-identifiers": [defaultContainer],
      "com.apple.developer.icloud-services": ["CloudKit"],
      "com.apple.developer.icloud-container-environment": ["Development", "Production"],
      "com.apple.developer.aps-environment": "production"
    }
  };
}

test("release uses only the matching production CloudKit identity", () => {
  const ent = cloudEntitlements(profile(), "TESTTEAM00");
  assert.equal(ent["com.apple.developer.icloud-container-environment"], "Production");
  assert.deepEqual(ent["com.apple.developer.icloud-container-identifiers"], [defaultContainer]);
  assert.equal(ent["com.apple.developer.aps-environment"], "production");
});

test("profile parsing supports native plist dates and certificate data", () => {
  const value = parseProfile(Buffer.from(`<?xml version="1.0"?><plist version="1.0"><dict>
    <key>ExpirationDate</key><date>2099-01-01T00:00:00Z</date>
    <key>DeveloperCertificates</key><array><data>ZmFrZQ==</data></array>
    <key>Platform</key><array><string>OSX</string></array>
  </dict></plist>`));
  assert.equal(value.ExpirationDate, "2099-01-01T00:00:00Z");
  assert.deepEqual(value.Platform, ["OSX"]);
  assert.equal(value.DeveloperCertificates, undefined);
});

test("rejects profiles that would ship a nonfunctional CloudKit application", () => {
  const edits = [
    (p) => { p.TeamIdentifier = ["DIFFERENT"]; },
    (p) => { p.ExpirationDate = "2000-01-01T00:00:00Z"; },
    (p) => { p.Platform = ["iOS"]; },
    (p) => { p.ProvisionsAllDevices = false; },
    (p) => { p.Entitlements["com.apple.application-identifier"] = "TESTTEAM00.another.app"; },
    (p) => { p.Entitlements["com.apple.developer.icloud-container-identifiers"] = ["iCloud.another.app"]; },
    (p) => { p.Entitlements["com.apple.developer.icloud-services"] = []; },
    (p) => { p.Entitlements["com.apple.developer.icloud-container-environment"] = "Development"; },
    (p) => { p.Entitlements["com.apple.developer.aps-environment"] = "development"; }
  ];
  for (const edit of edits) { const candidate = profile(); edit(candidate); assert.throws(() => cloudEntitlements(candidate, "TESTTEAM00")); }
});

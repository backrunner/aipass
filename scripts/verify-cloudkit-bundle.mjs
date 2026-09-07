import { verifyCloudKitBundle } from "./cloudkit-profile.mjs";
if (!process.argv[2] || !process.env.APPLE_TEAM_ID) throw new Error("Pass the app path and APPLE_TEAM_ID");
await verifyCloudKitBundle(process.argv[2], process.env.APPLE_TEAM_ID);
console.log("Verified embedded CloudKit profile and signed production entitlements.");

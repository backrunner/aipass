// Cargo's JSON output is authoritative; test executable paths are not stable.
export function testExecutable(messages, targetName) {
  const paths = new Set();
  for (const line of messages.split(/\r?\n/)) {
    let message;
    try { message = JSON.parse(line); } catch { continue; }
    if (message?.reason === "compiler-artifact" && message.profile?.test === true
      && message.target?.name === targetName && message.target.kind?.includes("lib")
      && typeof message.executable === "string") {
      paths.add(message.executable);
    }
  }
  if (paths.size !== 1) throw new Error(`Expected one Cargo library test executable for ${targetName}; found ${paths.size}`);
  return [...paths][0];
}

import { readSync, writeSync } from "node:fs";

function readBytes(size) {
  const buffer = Buffer.alloc(size);
  for (let offset = 0; offset < size;) {
    const count = readSync(0, buffer, offset, size - offset, null);
    if (count === 0) process.exit(0);
    offset += count;
  }
  return buffer;
}

function receive() {
  const size = readBytes(4).readUInt32BE();
  if (!size || size > 8 * 1024 * 1024) throw new Error("Invalid frame size");
  return JSON.parse(readBytes(size).toString("utf8"));
}

function send(type, fields = {}) {
  const payload = Buffer.from(JSON.stringify({ type, ...fields }));
  const length = Buffer.alloc(4);
  length.writeUInt32BE(payload.length);
  const frame = Buffer.concat([length, payload]);
  for (let offset = 0; offset < frame.length;) offset += writeSync(1, frame, offset, frame.length - offset);
}

const text = (en, zh) => ({ en, "zh-CN": zh });

function accountUI(account, { seats, region }) {
  return [{ title: text("Workspace", "工作区"), blocks: [
    { type: "status", label: text("Connection", "连接状态"), value: text("Connected", "已连接"), tone: "success" },
    { type: "facts", items: [
      { label: text("Workspace ID", "工作区 ID"), value: "ws:" + account, copyable: true },
      { label: text("Runtime", "运行环境"), value: "JavaScript" },
    ] },
    { type: "progress", label: text("Seat occupancy", "席位使用率"), percent: Math.round(1000 / seats) / 10 },
    { type: "table", columns: [text("Region", "区域"), text("Seats", "席位数")], rows: [[region, String(seats)]] },
    { type: "note", tone: "info", text: text("One seat is assigned to this account.", "此账号已占用一个席位。") },
  ] }];
}

function generate(request) {
  const stream = request.body.stream === true;
  const message = { role: "assistant", content: "Offline example response" };
  const base = { id: "example", model: request.model, created: 0, object: stream ? "chat.completion.chunk" : "chat.completion" };
  const response = (choice) => JSON.stringify({ ...base, choices: [{ index: 0, ...choice }] });
  send("headers", { status: 200, contentType: stream ? "text/event-stream" : "application/json" });
  const chunks = stream ? [
    "data: " + response({ delta: message, finish_reason: null }) + "\n\n",
    "data: " + response({ delta: {}, finish_reason: "stop" }) + "\n\ndata: [DONE]\n\n",
  ] : [response({ message, finish_reason: "stop" })];
  for (const chunk of chunks) {
    send("data", { base64: Buffer.from(chunk).toString("base64") });
    if (receive().op !== "ack") return;
  }
  send("end");
}

function main() {
  const hello = receive();
  if (hello.op !== "initialize" || hello.protocol !== 1) return;
  send("ready", { protocol: 1, nonce: hello.nonce });
  const request = receive();
  switch (request.op) {
    case "login": {
      if (request.method === 1) {
        send("challenge", { url: "", instructions: "Enter DEMO / 请输入 DEMO", manual: true });
        const reply = receive();
        if (reply.op !== "code" || reply.code !== "DEMO") { send("error", { code: "reconnect" }); return; }
      }
      const { account, seats, region } = request.inputs;
      send("connected", { accountId: account || "demo", state: JSON.stringify({ seats: Number(seats || 5), region: region || "local" }),
        models: [{ id: "offline", name: "Offline", wire: "chat" }] });
      break;
    }
    case "refresh":
      send("usage", { plan: "Offline plan", balance: "75", windows: [{ name: "Daily", used: 25, span: 86400 }],
        sections: accountUI(request.accountId, JSON.parse(request.state)) });
      break;
    case "generate": generate(request); break;
    case "shutdown": return;
    default: send("error", { code: "unsupported" });
  }
  receive(); // The host closes this one-operation session with shutdown or EOF.
}
main();

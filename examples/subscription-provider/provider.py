"""Offline AIPass provider. Use any dummy key. No network or files are used."""
import base64
import json
import struct
import sys


def receive():
    header = sys.stdin.buffer.read(4)
    if len(header) != 4:
        raise EOFError
    size, = struct.unpack(">I", header)
    if not 0 < size <= 8 * 1024 * 1024:
        raise ValueError("invalid frame size")
    data = sys.stdin.buffer.read(size)
    if len(data) != size:
        raise EOFError
    return json.loads(data)


def send(event, **fields):
    data = json.dumps({"type": event, **fields}).encode()
    sys.stdout.buffer.write(struct.pack(">I", len(data)) + data)
    sys.stdout.buffer.flush()


def chunk(data):
    send("data", base64=base64.b64encode(data.encode()).decode())
    return receive()["op"] == "ack"


def generate(request):
    answer = "Offline example response"
    stream = request["body"].get("stream", False)
    send("headers", status=200, contentType="text/event-stream" if stream else "application/json")
    response = {"id": "example", "model": request["model"], "created": 0}
    if stream:
        response["object"] = "chat.completion.chunk"
        response["choices"] = [{"index": 0, "delta": {"role": "assistant", "content": answer}, "finish_reason": None}]
        if not chunk("data: " + json.dumps(response) + "\n\n"):
            return
        response["choices"] = [{"index": 0, "delta": {}, "finish_reason": "stop"}]
        if not chunk("data: " + json.dumps(response) + "\n\ndata: [DONE]\n\n"):
            return
    else:
        response["object"] = "chat.completion"
        response["choices"] = [{"index": 0, "message": {"role": "assistant", "content": answer}, "finish_reason": "stop"}]
        if not chunk(json.dumps(response)):
            return
    send("end")


def localized(en, zh):
    return {"en": en, "zh-CN": zh}


def account_ui(account, settings):
    return [{"title": localized("Workspace", "工作区"), "blocks": [
        {"type": "status", "label": localized("Connection", "连接状态"),
         "value": localized("Connected", "已连接"), "tone": "success"},
        {"type": "facts", "items": [
            {"label": localized("Workspace ID", "工作区 ID"), "value": "ws:" + account, "copyable": True},
            {"label": localized("Runtime", "运行环境"), "value": "Python"}]},
        {"type": "progress", "label": localized("Seat occupancy", "席位使用率"),
         "percent": round(100 / settings["seats"], 1)},
        {"type": "table", "columns": [localized("Region", "区域"), localized("Seats", "席位数")],
         "rows": [[settings["region"], str(settings["seats"])]]},
        {"type": "note", "tone": "info", "text": localized("One seat is assigned to this account.", "此账号已占用一个席位。")},
    ]}]


def main():
    hello = receive()
    if hello["op"] != "initialize" or hello["protocol"] != 1:
        return
    send("ready", protocol=1, nonce=hello["nonce"])
    request = receive()
    if request["op"] == "shutdown":
        return
    if request["op"] == "login":
        inputs = request["inputs"]
        account = inputs.get("account") or "demo"
        if request["method"] == 1:
            send("challenge", url="", instructions="Enter DEMO / 请输入 DEMO", manual=True)
            reply = receive()
            if reply.get("op") != "code" or reply.get("code") != "DEMO":
                send("error", code="reconnect")
                return
        state = json.dumps({"seats": int(inputs.get("seats") or 5), "region": inputs.get("region") or "local"})
        send("connected", accountId=account, state=state,
             models=[{"id": "offline", "name": "Offline", "wire": "chat"}])
    elif request["op"] == "refresh":
        send("usage", plan="Offline plan", balance="75",
             windows=[{"name": "Daily", "used": 25, "span": 86400}],
             sections=account_ui(request["accountId"], json.loads(request["state"])))
    elif request["op"] == "generate":
        generate(request)
    else:
        send("error", code="unsupported")
    receive()  # shutdown


if __name__ == "__main__":
    try:
        main()
    except (EOFError, BrokenPipeError):
        pass

//! A self-contained offline subscription provider using only private stdio.
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::{json, Value};
use std::io::{self, Read, Write};

fn receive() -> io::Result<Value> {
    let mut input = io::stdin().lock();
    let mut size = [0; 4];
    input.read_exact(&mut size)?;
    let size = u32::from_be_bytes(size) as usize;
    if !(1..=8 * 1024 * 1024).contains(&size) {
        return Err(io::Error::other("Invalid frame size"));
    }
    let mut payload = vec![0; size];
    input.read_exact(&mut payload)?;
    Ok(serde_json::from_slice(&payload)?)
}

fn send(event: Value) -> io::Result<()> {
    let payload = serde_json::to_vec(&event)?;
    let mut out = io::stdout().lock();
    out.write_all(&(payload.len() as u32).to_be_bytes())?;
    out.write_all(&payload)?;
    out.flush()
}

fn text(en: &str, zh: &str) -> Value {
    json!({"en":en, "zh-CN":zh})
}

fn account_ui(account: &Value, settings: &Value) -> Value {
    let seats = settings["seats"].as_u64().unwrap_or(5).max(1);
    json!([{"title":text("Workspace","工作区"), "blocks":[
        {"type":"status", "label":text("Connection","连接状态"), "value":text("Connected","已连接"), "tone":"success"},
        {"type":"facts", "items":[{"label":text("Workspace ID","工作区 ID"),"value":format!("ws:{}",account.as_str().unwrap_or("demo")),"copyable":true},
            {"label":text("Runtime","运行环境"),"value":"Rust"}]},
        {"type":"progress", "label":text("Seat occupancy","席位使用率"), "percent":(1000.0/seats as f64).round()/10.0},
        {"type":"table", "columns":[text("Region","区域"),text("Seats","席位数")], "rows":[[settings["region"],seats.to_string()]]},
        {"type":"note", "tone":"info", "text":text("One seat is assigned to this account.","此账号已占用一个席位。")}
    ]}])
}

fn login(request: &Value) -> io::Result<()> {
    if request["method"] == 1 {
        send(
            json!({"type":"challenge","manual":true,"url":"","instructions":"Enter DEMO / 请输入 DEMO"}),
        )?;
        let reply = receive()?;
        if reply["op"] != "code" || reply["code"] != "DEMO" {
            return send(json!({"type":"error","code":"reconnect"}));
        }
    }
    let inputs = &request["inputs"];
    let seats = inputs["seats"]
        .as_str()
        .and_then(|n| n.parse::<u64>().ok())
        .unwrap_or(5);
    let settings = json!({"seats":seats, "region":inputs["region"].as_str().unwrap_or("local")});
    send(
        json!({"type":"connected","accountId":inputs["account"].as_str().unwrap_or("demo"),
        "state":settings.to_string(),"models":[{"id":"offline","name":"Offline","wire":"chat"}]}),
    )
}

fn generate(request: &Value) -> io::Result<()> {
    let stream = request["body"]["stream"] == true;
    let content = if stream {
        "text/event-stream"
    } else {
        "application/json"
    };
    send(json!({"type":"headers","status":200,"contentType":content}))?;
    let message = json!({"role":"assistant","content":"Offline example response"});
    let response = |choice: Value| {
        json!({"id":"example","model":request["model"],"created":0,
        "object":if stream {"chat.completion.chunk"} else {"chat.completion"}, "choices":[choice]})
    };
    let chunks = if stream {
        let first = response(json!({"index":0,"delta":message,"finish_reason":null}));
        let last = response(json!({"index":0,"delta":{},"finish_reason":"stop"}));
        vec![
            format!("data: {first}\n\n"),
            format!("data: {last}\n\ndata: [DONE]\n\n"),
        ]
    } else {
        vec![response(json!({"index":0,"message":message,"finish_reason":"stop"})).to_string()]
    };
    for chunk in chunks {
        send(json!({"type":"data","base64":STANDARD.encode(chunk)}))?;
        if receive()?["op"] != "ack" {
            return Ok(());
        }
    }
    send(json!({"type":"end"}))
}

fn run() -> io::Result<()> {
    let hello = receive()?;
    if hello["op"] != "initialize" || hello["protocol"] != 1 {
        return Ok(());
    }
    send(json!({"type":"ready","protocol":1,"nonce":hello["nonce"]}))?;
    let request = receive()?;
    match request["op"].as_str() {
        Some("login") => login(&request)?,
        Some("refresh") => {
            let settings = serde_json::from_str(request["state"].as_str().unwrap_or(""))?;
            send(
                json!({"type":"usage","plan":"Offline plan","balance":"75","windows":[{"name":"Daily","used":25,"span":86400}],
                "sections":account_ui(&request["accountId"], &settings)}),
            )?;
        }
        Some("generate") => generate(&request)?,
        Some("shutdown") => return Ok(()),
        _ => send(json!({"type":"error","code":"unsupported"}))?,
    }
    receive()?;
    Ok(())
}

fn main() -> io::Result<()> {
    match run() {
        Err(e)
            if matches!(
                e.kind(),
                io::ErrorKind::UnexpectedEof | io::ErrorKind::BrokenPipe
            ) =>
        {
            Ok(())
        }
        result => result,
    }
}

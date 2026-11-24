// A standalone, offline AIPass subscription provider.
package main

import (
	"encoding/base64"
	"encoding/binary"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"math"
	"os"
	"strconv"
)

type object = map[string]any
type call struct {
	Op        string
	Protocol  int
	Nonce     string
	Method    int
	Inputs    map[string]string
	Code      string
	AccountID string
	State     string
	Model     string
	Body      struct{ Stream bool }
}
type settings struct {
	Seats  int    `json:"seats"`
	Region string `json:"region"`
}

func receive() (call, error) {
	var request call
	var size uint32
	if err := binary.Read(os.Stdin, binary.BigEndian, &size); err != nil {
		return request, err
	}
	if size == 0 || size > 8*1024*1024 {
		return request, errors.New("invalid frame size")
	}
	payload := make([]byte, size)
	if _, err := io.ReadFull(os.Stdin, payload); err != nil {
		return request, err
	}
	err := json.Unmarshal(payload, &request)
	return request, err
}

func send(kind string, fields object) error {
	fields["type"] = kind
	payload, err := json.Marshal(fields)
	if err != nil {
		return err
	}
	if err := binary.Write(os.Stdout, binary.BigEndian, uint32(len(payload))); err != nil {
		return err
	}
	_, err = os.Stdout.Write(payload)
	return err
}

func text(en, zh string) object { return object{"en": en, "zh-CN": zh} }

func accountUI(account string, s settings) []object {
	return []object{{"title": text("Workspace", "工作区"), "blocks": []object{
		{"type": "status", "label": text("Connection", "连接状态"), "value": text("Connected", "已连接"), "tone": "success"},
		{"type": "facts", "items": []object{{"label": text("Workspace ID", "工作区 ID"), "value": "ws:" + account, "copyable": true}, {"label": text("Runtime", "运行环境"), "value": "Go"}}},
		{"type": "progress", "label": text("Seat occupancy", "席位使用率"), "percent": math.Round(1000/float64(s.Seats)) / 10},
		{"type": "table", "columns": []object{text("Region", "区域"), text("Seats", "席位数")}, "rows": [][]string{{s.Region, strconv.Itoa(s.Seats)}}},
		{"type": "note", "tone": "info", "text": text("One seat is assigned to this account.", "此账号已占用一个席位。")},
	}}}
}

func login(req call) error {
	if req.Method == 1 {
		if err := send("challenge", object{"manual": true, "url": "", "instructions": "Enter DEMO / 请输入 DEMO"}); err != nil {
			return err
		}
		reply, err := receive()
		if err != nil {
			return err
		}
		if reply.Op != "code" || reply.Code != "DEMO" {
			return send("error", object{"code": "reconnect"})
		}
	}
	seats, err := strconv.Atoi(req.Inputs["seats"])
	if err != nil || seats < 1 {
		seats = 5
	}
	region := req.Inputs["region"]
	if region == "" {
		region = "local"
	}
	state, err := json.Marshal(settings{seats, region})
	if err != nil {
		return err
	}
	return send("connected", object{"accountId": req.Inputs["account"], "state": string(state),
		"models": []object{{"id": "offline", "name": "Offline", "wire": "chat"}}})
}

func generate(req call) error {
	mime, kind := "application/json", "chat.completion"
	if req.Body.Stream {
		mime, kind = "text/event-stream", "chat.completion.chunk"
	}
	if err := send("headers", object{"status": 200, "contentType": mime}); err != nil {
		return err
	}
	message := object{"role": "assistant", "content": "Offline example response"}
	choices := []object{{"index": 0, "message": message, "finish_reason": "stop"}}
	if req.Body.Stream {
		choices = []object{{"index": 0, "delta": message, "finish_reason": nil}, {"index": 0, "delta": object{}, "finish_reason": "stop"}}
	}
	for i, choice := range choices {
		payload, err := json.Marshal(object{"id": "example", "model": req.Model, "created": 0, "object": kind, "choices": []object{choice}})
		if err != nil {
			return err
		}
		if req.Body.Stream {
			payload = []byte(fmt.Sprintf("data: %s\n\n", payload))
			if i == len(choices)-1 {
				payload = append(payload, "data: [DONE]\n\n"...)
			}
		}
		if err := send("data", object{"base64": base64.StdEncoding.EncodeToString(payload)}); err != nil {
			return err
		}
		reply, err := receive()
		if err != nil || reply.Op != "ack" {
			return err
		}
	}
	return send("end", object{})
}

func run() error {
	hello, err := receive()
	if err != nil || hello.Op != "initialize" || hello.Protocol != 1 {
		return err
	}
	if err := send("ready", object{"protocol": 1, "nonce": hello.Nonce}); err != nil {
		return err
	}
	req, err := receive()
	if err != nil {
		return err
	}
	switch req.Op {
	case "login":
		err = login(req)
	case "refresh":
		var s settings
		if err := json.Unmarshal([]byte(req.State), &s); err != nil {
			return err
		}
		err = send("usage", object{"plan": "Offline plan", "balance": "75", "windows": []object{{"name": "Daily", "used": 25, "span": 86400}},
			"sections": accountUI(req.AccountID, s)})
	case "generate":
		err = generate(req)
	case "shutdown":
		return nil
	default:
		err = send("error", object{"code": "unsupported"})
	}
	if err != nil {
		return err
	}
	_, err = receive()
	return err
}

func main() {
	if err := run(); err != nil && !errors.Is(err, io.EOF) && !errors.Is(err, io.ErrUnexpectedEOF) {
		os.Exit(1)
	}
}

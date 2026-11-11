"""验证接入实板的握手、配置事务、全量帧、切页和重连。"""
import json
import sys
import time
import unittest
import uuid

import serial

PORT = sys.argv.pop(1)


class UsbSmoke(unittest.TestCase):
    def receive(self, kind, request_id=None, retry=None):
        deadline = time.monotonic() + 12
        repeat = time.monotonic() + 2
        diagnostics = []
        while time.monotonic() < deadline:
            if retry and time.monotonic() >= repeat:
                self.port.write(retry)
                repeat = time.monotonic() + 2
            newline = self.input.find(b"\n")
            if newline < 0:
                self.input.extend(self.port.read(self.port.in_waiting or 1))
                continue
            line = self.input[:newline]
            del self.input[:newline + 1]
            try:
                message = json.loads(line)
            except ValueError:
                if line and len(diagnostics) < 60:
                    diagnostics.append(line.decode("utf-8", "replace").strip())
                continue
            if message.get("type") != kind:
                continue
            if request_id is None or message["payload"].get("replyTo") == request_id:
                return message
        self.fail(f"没有收到 {kind} ({request_id})\n" + "\n".join(diagnostics))

    def request(self, kind, payload):
        self.id += 1
        message = dict(protocolVersion=1, id=self.id, sessionId=self.session,
                       type=kind, payload=payload)
        self.wire = json.dumps(message, ensure_ascii=False).encode() + b"\n"
        self.port.write(b"\n" + self.wire)
        return self.receive("welcome" if kind == "hello" else "result", self.id, b"\n" + self.wire)

    def hello(self):
        self.id, self.session = 0, None
        welcome = self.request("hello", {
            "supportedProtocolVersions": [1], "appVersion": "smoke",
            "clientNonce": str(uuid.uuid4()),
        })
        self.assertEqual(welcome["payload"]["profileId"], "tdisplay-c5-v1")
        self.session = welcome["sessionId"]
        return welcome

    def ok(self, kind, payload):
        result = self.request(kind, payload)["payload"]
        self.assertTrue(result["ok"], result)
        return result["data"]

    def test_screen_rotation(self):
        with serial.Serial(PORT, 115200, timeout=0.25) as self.port:
            self.port.reset_input_buffer()
            self.input = bytearray()
            welcome = self.hello()
            self.assertIn("screenRotated", welcome["payload"]["capabilities"]["settings"])
            original = self.receive("device.status")["payload"]["screenRotated"]
            self.assertIs(type(original), bool)
            try:
                invalid = self.request("settings.set", {"screenRotated": 1})["payload"]
                self.assertEqual(invalid["error"]["code"], "INVALID_PAYLOAD")
                self.assertEqual(self.ok("settings.set", {"screenRotated": not original}),
                                 {"screenRotated": not original})
                self.assertIs(self.receive("device.status")["payload"]["screenRotated"], not original)
                self.port.dtr = False
                self.port.rts = True
                time.sleep(0.1)
                self.port.rts = False
                time.sleep(3)
                self.port.reset_input_buffer()
                self.input.clear()
                restarted = self.hello()
                self.assertNotEqual(restarted["payload"]["bootId"], welcome["payload"]["bootId"])
                self.assertIs(self.receive("device.status")["payload"]["screenRotated"], not original)
            finally:
                self.ok("settings.set", {"screenRotated": original})

    def test_transactions(self):
        with serial.Serial(PORT, 115200, timeout=0.25) as self.port:
            self.port.reset_input_buffer()
            self.input = bytearray()
            self.port.write(b"\n")
            welcome = self.hello()
            self.port.write(self.wire)
            self.assertEqual(self.receive("welcome", 1)["sessionId"], self.session)
            self.ok("time.sync", dict(unixMs=int(time.time() * 1000), utcOffsetMinutes=480))
            config = {
                "schemaVersion": 1, "profileId": "tdisplay-c5-v1",
                "theme": {
                    "active": "dark",
                    "dark": dict(background="#1e1e1e", surface="#4a4a4a", text="#f1f1f1"),
                    "light": dict(background="#e3e3e3", surface="#fafafa", text="#242424"),
                },
                "playback": dict(defaultPageId="codex", rotationSeconds=0),
                "pages": [
                    dict(pageId="codex", name="Codex 软件", cards=[
                        dict(cardId="quota", x=0, y=0, w=2, h=2, render=dict(
                            kind="progress", label="剩余额度", unit="%", numberAnimation="digitRoll")),
                        dict(cardId="task", x=2, y=0, w=2, h=1, render=dict(
                            kind="state", label="任务状态", supportsNotify=True,
                            stateLabels={"running": "运行中", "completed": "已完成"},
                            stateStyles={"completed": {"color": "#48b34a", "alertDurationMs": 2000}})),
                        dict(cardId="count", x=2, y=1, w=1, h=1, render=dict(
                            kind="metric", format="countdown", label="额度恢复")),
                    ]),
                    dict(pageId="system", name="系统", cards=[
                        dict(cardId="chart", x=0, y=0, w=1, h=1, render=dict(
                            kind="chart", label="CPU占用", unit="%",
                            chart=dict(sampleCount=12, rangeSeconds=3600, min=0, max=100))),
                        dict(cardId="trend", x=1, y=0, w=1, h=1, render=dict(
                            kind="trend", label="CPU 趋势", unit="%", trend=dict(deltaUnit="pp"))),
                        dict(cardId="range", x=2, y=0, w=1, h=1, render=dict(
                            kind="range", label="内存区间", unit="%", range=dict(mode="fixed", min=0, max=100))),
                    ]),
                ],
            }
            base = welcome["payload"]["configRevision"]
            data = self.ok("config.set", dict(baseRevision=base, config=config))
            revision = data["configRevision"]
            last_config = self.wire
            self.port.write(last_config)
            self.assertEqual(self.receive("result", self.id)["payload"]["data"], data)
            self.assertEqual(self.ok("config.set", dict(baseRevision=revision, config=config)), {
                "configRevision": revision, "changed": False,
            })
            now = int(time.time() * 1000)
            readings = {
                "quota": dict(kind="progress", value=81, ratio=0.81),
                "task": dict(kind="state", state="running", detail="实板显示验证：中文任务标题可以滚动"),
                "count": dict(kind="metric", deadlineUnixMs=now + 120_000),
                "chart": dict(kind="chart", windowEndUnixMs=now, values=list(range(10, 70, 5))),
                "trend": dict(kind="trend", value=23, delta=4),
                "range": dict(kind="range", value=42),
            }
            frame = dict(configRevision=revision, sequence=1, mode="snapshot", readings=readings)
            self.assertEqual(self.ok("readings.set", frame)["acceptedSequence"], 1)
            bad = self.request("readings.set", dict(frame, sequence=2, mode="update", readings={}))
            self.assertEqual(bad["payload"]["error"]["code"], "INVALID_PAYLOAD")
            readings["quota"]["value"] = 80
            readings["task"].update(state="completed", notify=True)
            update = dict(frame, sequence=2, mode="update")
            self.assertEqual(self.ok("readings.set", update)["acceptedSequence"], 2)
            self.port.write(self.wire)
            self.assertEqual(self.receive("result", self.id)["payload"]["data"]["acceptedSequence"], 2)
            self.assertEqual(self.request("readings.set", update)["payload"]["error"]["code"], "STALE_SEQUENCE")
            self.assertEqual(self.ok("page.set", dict(configRevision=revision, pageId="system"))["activePageId"], "system")
            self.assertEqual(self.ok("settings.set", dict(brightnessPercent=70))["brightnessPercent"], 70)
            self.ok("ping", {})
            time.sleep(16)
            next_welcome = self.hello()
            self.assertNotEqual(next_welcome["sessionId"], welcome["sessionId"])
            self.assertEqual(next_welcome["payload"]["bootId"], welcome["payload"]["bootId"])
            status = self.receive("device.status")["payload"]
            self.assertEqual(status["activePageId"], "system")
            self.assertIsNotNone(status["lastReadingsAppliedAtUnixMs"])
            self.assertEqual(status["configRevision"], revision)

            self.port.dtr = False
            self.port.rts = True
            time.sleep(0.1)
            self.port.rts = False
            time.sleep(3)
            self.port.reset_input_buffer()
            self.input.clear()
            restarted = self.hello()
            self.assertNotEqual(restarted["payload"]["bootId"], welcome["payload"]["bootId"])
            self.assertEqual(restarted["payload"]["configRevision"], revision)
            self.assertIsNone(self.receive("device.status")["payload"]["lastReadingsAppliedAtUnixMs"])

            # 覆盖 5 页 40 卡，标题跨 Unicode 分片且远宽于可见区。
            pages, full = [], {}
            for page_number in range(5):
                page_id = f"stress-{page_number}"
                cards = []
                for slot in range(8):
                    card_id = f"{page_id}-{slot}"
                    cards.append(dict(cardId=card_id, x=slot % 4, y=slot // 4, w=1, h=1,
                                      render=dict(kind="state", label="任务", stateLabels={"running": "运行中"})))
                    title = "".join(chr(0x4e00 + (slot * 133 + i * 27) % 19000) for i in range(280))
                    full[card_id] = dict(kind="state", state="running", detail=title)
                pages.append(dict(pageId=page_id, name=page_id, cards=cards))
            large = dict(config, pages=pages, playback=dict(defaultPageId="stress-0", rotationSeconds=0))
            try:
                started = time.monotonic()
                stress = self.ok("config.set", dict(baseRevision=revision, config=large))["configRevision"]
                self.assertEqual(self.ok("readings.set", dict(configRevision=stress, sequence=1,
                    mode="snapshot", readings=full))["acceptedSequence"], 1)
                self.assertEqual(self.ok("page.set", dict(configRevision=stress, pageId="stress-4"))["activePageId"], "stress-4")
                self.ok("ping", {})
                print(f"5 页 40 卡，配置、{len(json.dumps(full, ensure_ascii=False).encode()):,} 字节读数、切页和心跳用时 {time.monotonic() - started:.2f}s")
            finally:
                if "stress" in locals():
                    self.ok("config.set", dict(baseRevision=stress, config=config))


if __name__ == "__main__":
    unittest.main()

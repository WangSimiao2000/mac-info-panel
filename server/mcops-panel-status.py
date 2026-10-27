#!/usr/bin/env python3
"""Emit a small, read-only mc-ops dashboard snapshot as JSON."""

from __future__ import annotations

import datetime as dt
import json
import os
import shutil
import socket
import struct
import subprocess
import time
import urllib.request
from pathlib import Path

SCHEMA = 1
AGENT_HEALTH_URL = "http://127.0.0.1:8765/api/health"
MC_SERVICE = "mc-server.service"
AGENT_SERVICE = "mcops-agent.service"
GAME_DIR = Path.home() / "minecraft" / "server"


def service_active(unit: str) -> bool:
    try:
        result = subprocess.run(
            ["systemctl", "--user", "is-active", "--quiet", unit],
            stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            timeout=2,
            check=False,
        )
        return result.returncode == 0
    except (OSError, subprocess.TimeoutExpired):
        return False


def agent_health() -> dict[str, object]:
    try:
        with urllib.request.urlopen(AGENT_HEALTH_URL, timeout=2) as response:
            payload = json.load(response)
        return {
            "health": str(payload.get("status", "unknown")),
            "version": payload.get("version"),
            "serviceActive": service_active(AGENT_SERVICE),
        }
    except Exception:
        return {"health": "unreachable", "version": None, "serviceActive": service_active(AGENT_SERVICE)}


def read_varint(stream) -> int:
    value = 0
    for index in range(5):
        byte = stream.read(1)
        if not byte:
            raise EOFError("unexpected EOF")
        current = byte[0]
        value |= (current & 0x7F) << (7 * index)
        if not current & 0x80:
            return value
    raise ValueError("VarInt too long")


def varint(value: int) -> bytes:
    value &= 0xFFFFFFFF
    out = bytearray()
    while True:
        current = value & 0x7F
        value >>= 7
        if value:
            current |= 0x80
        out.append(current)
        if not value:
            return bytes(out)


def minecraft_port() -> int:
    properties = GAME_DIR / "server.properties"
    try:
        for line in properties.read_text(encoding="utf-8").splitlines():
            if line.startswith("server-port="):
                port = int(line.split("=", 1)[1])
                if 1 <= port <= 65535:
                    return port
    except (OSError, ValueError):
        pass
    return 25565


def minecraft_status() -> dict[str, object]:
    active = service_active(MC_SERVICE)
    result: dict[str, object] = {
        "state": "running" if active else "offline",
        "serviceActive": active,
        "online": False,
        "playersOnline": None,
        "playersMax": None,
        "version": None,
        "uptimeSeconds": None,
    }
    port = minecraft_port()
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=1.5) as sock:
            stream = sock.makefile("rb")
            host = b"127.0.0.1"
            handshake = varint(0) + varint(760) + varint(len(host)) + host + struct.pack(">H", port) + varint(1)
            sock.sendall(varint(len(handshake)) + handshake + b"\x01\x00")
            _ = read_varint(stream)
            packet_id = read_varint(stream)
            if packet_id != 0:
                raise ValueError("unexpected status packet")
            length = read_varint(stream)
            payload = json.loads(stream.read(length).decode("utf-8"))
        players = payload.get("players") or {}
        version = payload.get("version") or {}
        result.update(
            state="online",
            online=True,
            playersOnline=players.get("online"),
            playersMax=players.get("max"),
            version=version.get("name"),
        )
    except Exception:
        pass
    return result


def cpu_counters() -> tuple[int, int]:
    fields = Path("/proc/stat").read_text(encoding="ascii").splitlines()[0].split()[1:]
    values = [int(value) for value in fields]
    idle = values[3] + (values[4] if len(values) > 4 else 0)
    return sum(values) - idle, sum(values)


def cpu_percent() -> float | None:
    try:
        used1, total1 = cpu_counters()
        time.sleep(0.15)
        used2, total2 = cpu_counters()
        total_delta = total2 - total1
        return round((used2 - used1) / total_delta * 100, 1) if total_delta > 0 else None
    except (OSError, ValueError, IndexError):
        return None


def memory_percent() -> float | None:
    try:
        values: dict[str, int] = {}
        for line in Path("/proc/meminfo").read_text(encoding="ascii").splitlines():
            key, value = line.split(":", 1)
            values[key] = int(value.strip().split()[0])
        total = values["MemTotal"]
        available = values.get("MemAvailable", values.get("MemFree", 0))
        return round((total - available) / total * 100, 1) if total else None
    except (OSError, ValueError, KeyError):
        return None


def disk_percent() -> float | None:
    try:
        usage = shutil.disk_usage(GAME_DIR if GAME_DIR.exists() else Path.home())
        return round(usage.used / usage.total * 100, 1) if usage.total else None
    except OSError:
        return None


def host_status() -> dict[str, object]:
    try:
        load1 = round(os.getloadavg()[0], 2)
    except OSError:
        load1 = None
    try:
        uptime = float(Path("/proc/uptime").read_text(encoding="ascii").split()[0])
    except (OSError, ValueError, IndexError):
        uptime = None
    return {
        "cpuPercent": cpu_percent(),
        "memoryPercent": memory_percent(),
        "diskPercent": disk_percent(),
        "load1": load1,
        "uptimeSeconds": uptime,
    }


def main() -> None:
    snapshot = {
        "schema": SCHEMA,
        "generatedAt": dt.datetime.now(dt.timezone.utc).isoformat(),
        "agent": agent_health(),
        "minecraft": minecraft_status(),
        "host": host_status(),
        "warning": None,
    }
    print(json.dumps(snapshot, ensure_ascii=False, separators=(",", ":")))


if __name__ == "__main__":
    main()

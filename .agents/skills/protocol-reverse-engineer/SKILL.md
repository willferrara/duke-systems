---
name: protocol-reverse-engineer
description: Decode binary formats, USB HID reports, and REST/WebSocket APIs into clean, zero-copy Rust parsers and serde structs.
---
# Protocol & Binary Reverse Engineering
1. Map raw HID byte offsets, bitmasks, and signed/unsigned endianness into fixed-size stack buffers (`[u8; 64]`) with unit tests.
2. Convert captured JSON API responses into strongly typed `#[derive(Serialize, Deserialize)]` Rust structs with `#[serde(default)]` resilience.
3. Document reverse-engineered packet layouts and endpoints in `AGENTS.md` or `docs/PROTOCOL.md`.

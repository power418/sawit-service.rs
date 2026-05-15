# Sawit Service / Sawit Engine Multiplayer (Rust) — Docs

Repo ini berisi dokumen PRD + rancangan arsitektur fondasi multiplayer scalable untuk Sawit Engine berbasis Rust (server authoritative, snapshot replication, interest management).

## Dokumen

- PRD utama: [prd_multiplayer_sawit_engine.md](prd_multiplayer_sawit_engine.md)
- Analogi arsitektur TCP/UDP scalable: [tcp_udp_scalable_architecture.md](tcp_udp_scalable_architecture.md)

## TL;DR

- Model: server authoritative + client prediction + snapshot interpolation.
- Skala awal: room-based session (8–16 pemain/room), lalu berkembang ke multi-room/multi-region.
- Kontrak data: tipe packet/schema ada di crate bersama `sawit_protocol` supaya client/server konsisten.

## Target MVP (ringkas)

Detail lengkap ada di PRD (bagian “Acceptance Criteria MVP”).

- Jalankan realtime server lokal.
- Jalankan 2 instance client dan join room yang sama.
- Pergerakan antar player terlihat real-time dan remote smooth.
- Server authoritative, client memakai prediction + reconciliation.
- Minimal 1 world action/event tersinkron (mis. interact/place/remove).
- Disconnect/reconnect aman dan ada debug ping/snapshot rate.

## Catatan encoding

Dokumen ditulis dalam UTF-8. Kalau muncul karakter aneh (mis. “â€””), pastikan editor/terminal membaca file sebagai UTF-8.

## Kontribusi

- Update spesifikasi di [prd_multiplayer_sawit_engine.md](prd_multiplayer_sawit_engine.md).
- Jaga penomoran section dan istilah agar tidak drift antar perubahan.

## Menjalankan (MVP)

- Run server UDP: `cargo run --bin sawit-service -- 0.0.0.0:4000`
- TCP control plane ikut hidup default di `0.0.0.0:4001`.
- Run client simulator (terminal lain): `cargo run --bin client_sim -- 127.0.0.1:4000 1 alice`
- Run Sawit Engine, lalu buka instance kedua untuk melihat remote player di room yang sama.

Env opsional:

- `SAWIT_BIND=0.0.0.0:4000`
- `SAWIT_PUBLIC_UDP=127.0.0.1:4000`
- `SAWIT_TCP_BIND=0.0.0.0:4001`
- `SAWIT_TCP_BIND=off` untuk mematikan TCP control plane
- `SAWIT_TICK_HZ=20`
- `SAWIT_SNAPSHOT_HZ=20`
- `SAWIT_MOVE_SPEED=6.2`
- `SAWIT_TIMEOUT_SECS=10`

## TCP Control Plane

TCP dipakai sebagai control plane ringan, seperti lobby/service-discovery untuk client sebelum masuk jalur UDP realtime. Protocol saat ini newline text:

```text
JOIN <room_id> <name>
ROOMS
HEALTH
HELP
QUIT
```

Contoh PowerShell:

```powershell
$tcp = [System.Net.Sockets.TcpClient]::new("127.0.0.1", 4001)
$stream = $tcp.GetStream()
$writer = [System.IO.StreamWriter]::new($stream)
$reader = [System.IO.StreamReader]::new($stream)
$reader.ReadLine()
$writer.WriteLine("JOIN 1 alice")
$writer.Flush()
$reader.ReadLine()
```

Response `JOIN_OK` berisi `udp_addr`, `room_id`, `join_token`, `tick_rate_hz`, dan `snapshot_rate_hz`. Untuk MVP, `join_token=dev-local` masih placeholder; gameplay realtime tetap masuk lewat UDP.

## Integrasi Sawit Engine C

`sawit-service` sekarang tetap menerima packet Rust/postcard untuk `client_sim`, dan juga menerima packet teks UDP ringan dari `sawit-engine`:

- `HELLO <protocol_version> <room_id> <name>`
- `INPUT <seq> <client_tick> <dt_ms> <move_x> <move_z> <yaw> <pitch>`
- `PING <client_time_ms>`
- `DISCONNECT`

Response teks yang dikirim ke engine:

- `WELCOME <protocol_version> <player_id> <room_id> <tick_hz> <snapshot_hz>`
- `SNAPSHOT <tick> <room_id> <count> ...player state`
- `PONG <client_time_ms> <server_time_ms>`
- `ERROR <code> <message>`

Default engine akan connect ke `127.0.0.1:4000`, room `1`. Override dari sisi engine:

- `SAWIT_SERVICE_ADDR=127.0.0.1:4000`
- `SAWIT_ROOM_ID=1`
- `SAWIT_PLAYER_NAME=nama_player`
- `SAWIT_MULTIPLAYER=0` untuk mematikan koneksi multiplayer.

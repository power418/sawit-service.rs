# Scalable TCP/UDP Architecture Analogy

Dokumen ini menjelaskan `sawit-service` seolah-olah ia adalah kumpulan microservice, tetapi dipakai untuk game server player-side yang realtime. Bedanya dengan microservice web biasa: bagian gameplay tidak boleh terlalu stateless, karena satu player dan satu room butuh authority yang konsisten setiap tick.

## Analogi Besar

Bayangkan game server seperti kota multiplayer:

- TCP adalah kantor administrasi kota.
- UDP adalah jalan raya realtime.
- Room worker adalah stadion/arena tempat pertandingan berlangsung.
- Player session adalah kartu akses pemain.
- Snapshot sender adalah kamera siaran yang mengirim keadaan arena ke semua pemain.
- Metrics/logging adalah ruang monitoring.

TCP mengurus hal yang perlu rapi, reliable, dan terurut. UDP mengurus hal yang perlu cepat dan boleh melepas packet lama.

## Dua Jalur Utama

### TCP Control Plane

TCP cocok untuk request yang sifatnya administratif:

- login/auth token;
- create room;
- join room;
- reconnect request;
- list room;
- chat/lobby ringan;
- inventory atau data persistent;
- server discovery;
- token refresh.

Analogi: TCP itu resepsionis. Player datang, dicek identitasnya, diberi nomor room, diberi alamat UDP server, lalu diarahkan masuk ke arena.

TCP tidak menjalankan movement tick. Kalau TCP ikut mengurus movement, latency dan head-of-line blocking akan terasa di gameplay.

### UDP Realtime Data Plane

UDP cocok untuk:

- input movement;
- camera yaw/pitch;
- action realtime;
- ping/pong;
- world snapshot;
- correction;
- unreliable event yang bisa diganti snapshot berikutnya.

Analogi: UDP itu jalur express di dalam arena. Tiap player melempar input ke room worker, lalu room worker mengirim snapshot balik berkala. Packet lama tidak perlu dikejar kalau snapshot baru sudah datang.

## Bentuk Microservice Game-Side

Secara konsep bisa dipecah seperti ini:

```text
Client
  |
  | TCP: auth/lobby/join/reconnect
  v
+------------------+
| TCP Gateway      |
+------------------+
  |
  | validates token, allocates room
  v
+------------------+        +------------------+
| Session Service  | <----> | Room Directory   |
+------------------+        +------------------+
  |
  | returns udp_addr + join_token + room_id
  v
Client
  |
  | UDP: hello/input/ping
  v
+------------------+
| UDP Gateway      |
+------------------+
  |
  | routes by room_id/player_id
  v
+------------------+
| Room Worker      |
| - tick loop      |
| - player state   |
| - validation     |
| - world state    |
+------------------+
  |
  | snapshots/events
  v
+------------------+
| Snapshot Fanout  |
+------------------+
  |
  v
Client
```

Di tahap MVP sekarang, beberapa kotak masih hidup dalam satu binary `sawit-service`. Tetapi mental model-nya sudah bisa dipakai supaya nanti gampang dipecah.

## Kenapa Gameplay Worker Stateful?

Microservice web sering dibuat stateless supaya gampang scale horizontal. Game realtime beda:

- posisi player harus punya satu sumber kebenaran;
- input sequence harus diproses berurutan per player;
- room/world mutation tidak boleh diproses oleh dua server berbeda secara bersamaan;
- snapshot harus berasal dari tick room yang sama.

Jadi yang stateless adalah gateway/control service. Yang stateful adalah room worker.

Rule penting:

```text
1 room aktif = 1 authoritative room worker
```

Kalau butuh scale, jangan bagi satu room kecil ke banyak server dulu. Bagi berdasarkan banyak room:

```text
Room 1 -> Game Server A
Room 2 -> Game Server A
Room 3 -> Game Server B
Room 4 -> Game Server C
```

## TCP Service Responsibilities

TCP layer sebaiknya tidak tahu detail physics. Ia hanya mengurus pintu masuk:

1. Client login atau minta join room.
2. TCP Gateway validasi identity/token.
3. Session Service membuat `session_id` dan short-lived `join_token`.
4. Room Directory memilih room worker yang tepat.
5. Client menerima:
   - `udp_addr`;
   - `room_id`;
   - `join_token`;
   - config dasar seperti tick/snapshot rate.

Contoh response join:

```text
{
  "udp_addr": "game-01.example.com:4000",
  "room_id": 42,
  "join_token": "short-lived-token",
  "tick_rate_hz": 20,
  "snapshot_rate_hz": 20
}
```

## UDP Service Responsibilities

UDP layer mengurus traffic panas:

1. Menerima `Hello(room_id, join_token)`.
2. Validasi token ringan atau cek cache session.
3. Bind `SocketAddr` ke `(room_id, player_id)`.
4. Menerima `InputCommand`.
5. Route input ke room worker.
6. Room worker menjalankan tick authoritative.
7. Snapshot dikirim balik sesuai interest player.

UDP tidak boleh query database tiap packet. Kalau perlu data session, cache di memory/Redis.

## Room Worker Responsibilities

Room worker adalah authority gameplay:

- menyimpan `PlayerState`;
- memproses input sequence;
- clamp movement;
- menjalankan simulation tick;
- validasi speed/action;
- menyusun snapshot;
- membersihkan timeout/disconnect;
- menentukan interest set.

Untuk MVP, `RoomState` masih map biasa di process. Untuk scalable stage, room worker bisa menjadi actor/task:

```text
RoomWorker {
  room_id,
  players,
  input_queue,
  world_state,
  tick_loop,
  snapshot_rate
}
```

## Routing UDP yang Scalable

Ada dua pendekatan.

### 1. Sticky Room Per Process

Client langsung connect ke UDP address milik game server yang memegang room.

Kelebihan:

- sederhana;
- latency rendah;
- tidak perlu hop tambahan;
- cocok untuk MVP sampai stage menengah.

Kekurangan:

- room migration lebih sulit;
- reconnect perlu room directory.

### 2. UDP Gateway + Internal Room Workers

Client connect ke UDP Gateway, lalu gateway route packet ke room worker internal.

Kelebihan:

- alamat public lebih stabil;
- gateway bisa rate limit dan filter abuse;
- room worker bisa private network.

Kekurangan:

- ada hop tambahan;
- harus hati-hati supaya gateway tidak jadi bottleneck.

Rekomendasi Sawit:

```text
MVP: direct UDP to game server process
Next: direct UDP + Room Directory
Later: UDP Gateway only if perlu edge routing, DDoS filtering, atau multi-region routing
```

## Flow Join Player

```text
1. Client -> TCP Gateway:
   Join room 42

2. TCP Gateway -> Session/Room Directory:
   Validate player and allocate room worker

3. TCP Gateway -> Client:
   udp_addr=127.0.0.1:4000, room_id=42, join_token=abc

4. Client -> UDP Server:
   Hello(protocol_version, room_id, join_token, name)

5. UDP Server:
   Bind addr -> player_id

6. Client -> UDP Server:
   Input(seq, movement, yaw, pitch)

7. Room Worker:
   Simulate tick, validate movement

8. UDP Server -> Client:
   Snapshot(tick, players, last_processed_input)
```

## Flow In-Game Player

Saat player menekan W:

```text
Client local prediction:
  player langsung bergerak di layar

UDP input:
  client kirim movement.z=1 + yaw/pitch

Server authority:
  room worker hitung posisi final

Snapshot:
  server kirim posisi authoritative

Client reconciliation:
  local player disesuaikan ke server snapshot

Remote interpolation:
  player lain dirender dari buffer snapshot
```

Ini seperti kasir cepat di arena:

- player memberi instruksi cepat lewat UDP;
- room worker adalah wasit;
- snapshot adalah pengumuman resmi posisi pemain.

## Scaling Roadmap untuk TCP/UDP

### Stage 1: Single Binary

Status sekarang:

- UDP server;
- room/player map in-memory;
- postcard + text wire format;
- basic snapshot broadcast.

Target:

- stabil untuk local multiplayer;
- mudah debug;
- belum perlu service discovery.

### Stage 2: TCP Lobby + UDP Game Server

Tambah:

- `sawit_lobby` TCP/HTTP service;
- create/list/join room;
- join token;
- game server register ke lobby.

Gameplay tetap di UDP game server.

### Stage 3: Multi Game Server

Tambah:

- Room Directory;
- Redis active room registry;
- game server heartbeat;
- player reconnect lookup.

Contoh registry:

```text
room:42 -> game-01:4000
room:43 -> game-02:4000
player:99 -> room:42
```

### Stage 4: UDP Gateway / Edge

Tambah jika perlu:

- public UDP edge;
- rate limiting;
- region routing;
- packet budget;
- anti-abuse;
- forwarding ke private room workers.

### Stage 5: Interest Management

UDP snapshot tidak lagi mengirim semua player/entity:

- kirim player dekat;
- kirim chunk subscribed;
- kirim world event relevan;
- delta snapshot.

## Prinsip Penting

- TCP untuk control plane.
- UDP untuk gameplay realtime.
- Room worker stateful dan authoritative.
- Scale horizontal berdasarkan room, bukan membelah satu player ke banyak service.
- Database tidak disentuh per tick.
- Gateway boleh stateless; room simulation tidak.
- Snapshot adalah sumber kebenaran untuk client.
- Input adalah request, bukan final position.

## Mapping ke Kode Saat Ini

Arsitektur dokumen ini sudah mulai dipindahkan ke kode Rust:

- `src/server.rs`
  - orchestration utama;
  - membuat shared `RoomDirectory`;
  - start TCP control gateway;
  - start UDP realtime gateway.

- `src/tcp.rs`
  - control plane TCP;
  - command `JOIN`, `ROOMS`, `HEALTH`, `HELP`, `QUIT`;
  - mengembalikan `udp_addr`, `room_id`, placeholder `join_token`, dan rate config.

- `src/udp.rs`
  - realtime data plane UDP;
  - menerima packet client;
  - route packet ke `RoomDirectory`;
  - menjalankan tick loop dan snapshot fanout.

- `src/room.rs`
  - stateful room directory;
  - player registry;
  - authoritative `PlayerState`;
  - timeout cleanup;
  - snapshot build.

- `src/simulation.rs`
  - movement validation dasar;
  - movement velocity yaw-relative.

- `src/wire.rs`
  - decode postcard client;
  - decode text UDP client untuk `sawit-engine` C;
  - encode postcard/text server packet.

- `src/protocol.rs`
  - kontrak packet Rust/postcard.

Struktur ini belum full distributed microservice, tapi boundary-nya sudah dipisahkan seperti production path: TCP mengatur masuknya player, UDP membawa input dan snapshot, room worker/directory memegang authority.

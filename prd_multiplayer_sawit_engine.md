# PRD Multiplayer Scalable Architecture — Sawit Engine Rust

> Sumber-kebenaran: dokumen ini; ringkasan repo ada di `README.md`.
> Terakhir diperbarui: 2026-05-15 (Asia/Jakarta).
> Encoding: UTF-8 (kalau muncul karakter aneh seperti “â€””, set editor/terminal ke UTF-8).

## 1. Ringkasan

Sawit Engine akan diposisikan sebagai game/engine berbasis Rust dengan arsitektur multiplayer scalable. Fokus PRD ini adalah membuat fondasi agar player yang dibuat di project terpisah tetap bisa saling connect, masuk room/server yang sama, sinkron posisi, aksi, dan state dunia secara real-time.

Pendekatan utama:

- Semua core baru dibuat di Rust.
- Game client Rust menangani renderer, input, prediction, interpolation, dan state visual.
- Realtime game server Rust menjadi authority utama untuk player state, world state, room/session, snapshot replication, validation, dan anti-cheat dasar.
- Protocol, math/state types, dan serialization dibuat sebagai shared Rust crate agar client dan server tidak beda definisi data.
- Backend scalable dipisah dari realtime server: auth, lobby, matchmaking, persistence, metrics.

Catatan istilah:

- **Sawit Engine**: client/engine (rendering, input, prediction, world view).
- **Sawit Service**: realtime authoritative game server (room/session, tick, snapshot, validation) + integrasi ke lobby/auth.

## 2. Masalah yang Ingin Diselesaikan

Saat game dibuat dari scratch dan modul player/game dibuat terpisah, masalah utama biasanya:

1. Player antar client tidak punya sumber kebenaran yang sama.
2. State gerakan, posisi, rotasi, aksi, dan world mutation gampang desync.
3. Kalau peer-to-peer langsung, cheating dan NAT traversal lebih ribet.
4. Kalau semua state dikirim ke semua orang, tidak scalable.
5. Kalau client dipercaya penuh, multiplayer gampang dimanipulasi.

Solusi yang direkomendasikan adalah server authoritative dengan snapshot replication dan interest management.

## 3. Tujuan Produk

Membuat multiplayer foundation untuk Sawit Engine Rust agar:

- Player bisa connect ke server.
- Player bisa join room/world yang sama.
- Player bisa melihat player lain secara real-time.
- Movement tetap smooth meskipun ada latency.
- Aksi world seperti place/remove/interact tersinkron.
- Server bisa scale dari prototype kecil sampai multi-room/multi-region.

## 4. Non-Tujuan Tahap Awal

- MMO ribuan player dalam satu world sejak MVP.
- Full authoritative physics kompleks.
- Anti-cheat level kompetitif.
- Marketplace/economy/inventory kompleks.
- Voice chat.
- Dedicated global matchmaking advanced.
- World persistence skala besar langsung dari awal.

## 5. Observasi dari Repo Target

Repo target mengarah ke engine/game development dengan rendering OpenGL, CMake, JavaScript/WebAssembly-related files, dan banyak modul engine seperti player controller, platform support, diagnostics, console overlay, rendering, dan world-related files. Fork `power418/sawit-engine.c` juga berasal dari `Programmer-nosleep/sawit-engine`.

Catatan penting untuk versi Rust:

- Repo lama bisa dijadikan referensi struktur fitur, bukan fondasi bahasa final.
- Untuk Rust, sebaiknya pecah menjadi workspace multi-crate.
- Jangan campur renderer, networking, protocol, dan server dalam satu crate besar.
- Shared data model harus ada agar client/server konsisten.

## 6. Prinsip Arsitektur

1. Server authoritative.
   Server adalah sumber kebenaran untuk posisi final, world mutation, damage, inventory, dan event penting.

2. Client responsive.
   Client tetap melakukan local prediction agar input terasa instan.

3. Snapshot + interpolation.
   Remote player tidak digerakkan langsung dari packet mentah, tapi dari buffer snapshot yang diinterpolasi.

4. Shared protocol crate.
   Client dan server memakai tipe data yang sama dari crate bersama.

5. Room/session isolation.
   Satu room/world bisa dipindahkan ke process/server berbeda.

6. Interest management.
   Server hanya mengirim entity/world state yang relevan berdasarkan jarak, chunk, atau area.

7. Async-first.
   Rust server memakai async runtime untuk connection handling, tetapi simulation tick tetap deterministic dan terkontrol.

## 7. High-Level Architecture

Arsitektur multiplayer modern yang sering dipakai game engine terkenal pada dasarnya memakai pola yang mirip:

- Unreal Engine → dedicated authoritative server.
- Riot Games → isolated game server per match.
- Valve Source Engine → snapshot replication + interpolation.
- Minecraft server architecture → room/world authority.
- Photon/FishNet/Mirror → room/session-based replication.
- Rust game server (Facepunch) → entity replication + interest management.

Perbedaannya biasanya hanya di:

- cara replication,
- transport,
- orchestration,
- persistence,
- dan interest management.

Untuk Sawit Engine Rust, target arsitektur yang direkomendasikan:

```text
+------------------------------------------------+
| Rust Game Client                               |
|------------------------------------------------|
| Renderer / Window / Input                      |
| Local Prediction                               |
| Snapshot Interpolation                         |
| World Renderer                                 |
| TCP/UDP Net Client                             |
+-------------------------+----------------------+
                          |
                          |
                 UDP -> gameplay realtime
                 TCP -> auth/chat/lobby
                          |
                          v
+-------------------------+----------------------+
| sawit-service.rs                                |
|------------------------------------------------|
| TCP Gateway                                     |
| UDP Realtime Socket                             |
| Session Manager                                 |
| Room Manager                                    |
| Tick Scheduler                                  |
| Snapshot Replication                            |
| Interest Management                             |
| Validation / Anti Cheat                         |
+-------------------------+----------------------+
                          |
                          |
        +-----------------+----------------+
        |                                  |
        v                                  v
+---------------+               +-------------------+
| Redis Cache   |               | PostgreSQL        |
|---------------|               |-------------------|
| Presence       |               | Accounts          |
| Session Cache  |               | Match History     |
| Room Registry  |               | World Save        |
| Pub/Sub        |               | Persistent Data   |
+---------------+               +-------------------+
```

### Kenapa Dipisah TCP dan UDP?

Pola ini sangat umum di game engine/network stack modern.

#### UDP

Dipakai untuk:

- movement,
- transform replication,
- realtime combat,
- snapshot,
- physics sync,
- interpolation stream.

Karena:

- latency lebih rendah,
- tidak blocking,
- packet lama boleh dibuang.

#### TCP

Dipakai untuk:

- login,
- auth,
- inventory,
- lobby,
- chat,
- persistence,
- transactional request.

Karena:

- reliable,
- ordered,
- cocok untuk data penting.

## 7.1 Arsitektur `sawit-service.rs`

`sawit-service.rs` menjadi entry point utama backend realtime.

Tujuan file/service ini:

- menerima koneksi TCP,
- menerima realtime packet UDP,
- maintain room/session,
- menjalankan simulation tick,
- broadcast snapshot,
- validasi player,
- sinkronisasi state.

Struktur service yang direkomendasikan:

```text
sawit-service.rs
│
├── tcp/
│   ├── auth_listener.rs
│   ├── lobby_listener.rs
│   ├── tcp_session.rs
│   └── packet_dispatch.rs
│
├── udp/
│   ├── udp_socket.rs
│   ├── snapshot_sender.rs
│   ├── packet_receiver.rs
│   ├── reliability.rs
│   └── congestion.rs
│
├── room/
│   ├── room_manager.rs
│   ├── room_instance.rs
│   ├── player_registry.rs
│   └── interest_management.rs
│
├── simulation/
│   ├── tick_scheduler.rs
│   ├── movement_system.rs
│   ├── world_system.rs
│   ├── physics_validation.rs
│   └── replication.rs
│
├── persistence/
│   ├── redis_cache.rs
│   ├── postgres.rs
│   └── save_queue.rs
│
├── metrics/
│   ├── tracing.rs
│   ├── telemetry.rs
│   └── profiler.rs
│
└── main.rs
```

## 7.2 Arsitektur yang Sering Dipakai Game Multiplayer Besar

### Dedicated Server Architecture

Dipakai oleh:

- Counter Strike
- Valorant
- PUBG
- Apex
- Rust
- Minecraft Dedicated Server

Pola:

```text
Client
  -> Dedicated Game Server
       -> authoritative simulation
```

Kelebihan:

- anti-cheat lebih bagus,
- sinkronisasi stabil,
- scalable.

Kekurangan:

- perlu biaya server.

Rekomendasi: gunakan ini.

### Room-Based Session Architecture

Dipakai oleh:

- MOBA,
- FPS competitive,
- co-op session game.

Pola:

```text
1 match = 1 isolated room instance
```

Kelebihan:

- gampang scaling horizontal,
- crash satu room tidak mempengaruhi room lain,
- orchestration lebih mudah.

Cocok untuk Sawit Engine.

### ECS + Replication Architecture

Dipakai oleh:

- Unity DOTS NetCode
- Bevy ECS networking
- Overwatch internal architecture

Pola:

```text
Entity
  -> Component
      -> replicated selectively
```

Kelebihan:

- scalable untuk entity besar,
- replication granular.

Untuk Sawit Engine Rust, disarankan world/player nanti bergerak ke ECS-friendly architecture.

### Snapshot + Interpolation Architecture

Dipakai oleh:

- Source Engine,
- Valve multiplayer,
- Quake lineage,
- banyak FPS modern.

Pola:

```text
Server kirim snapshot berkala
Client render sedikit terlambat
Client interpolate antar snapshot
```

Kelebihan:

- movement remote smooth,
- packet loss lebih toleran.

Ini wajib dipakai.

### Client Prediction + Reconciliation

Dipakai oleh:

- Valorant
- Apex
- Counter Strike
- Titanfall

Pola:

```text
Client langsung gerak lokal
Server kirim posisi authority
Client replay pending input
```

Kelebihan:

- input terasa instant.

Ini wajib untuk movement modern.

## 7.3 Target Final Sawit Multiplayer Architecture

Arsitektur final yang ditargetkan:

```text
                    +----------------+
                    | Auth Service   |
                    +----------------+
                             |
                    +----------------+
                    | Lobby Service  |
                    +----------------+
                             |
                  Allocate Room/Match
                             |
        +--------------------------------------+
        |                                      |
        v                                      v
+-------------------+              +-------------------+
| Game Server A     |              | Game Server B     |
|-------------------|              |-------------------|
| Room 1            |              | Room 3            |
| Room 2            |              | Room 4            |
+-------------------+              +-------------------+
        |                                      |
        +----------------+---------------------+
                         |
                 +---------------+
                 | Redis / Queue |
                 +---------------+
                         |
                 +---------------+
                 | PostgreSQL    |
                 +---------------+
```

Gameplay realtime hanya hidup di game server.

Backend service lain:

- tidak menjalankan physics,
- tidak menjalankan movement,
- tidak menjalankan snapshot realtime.

Mereka hanya support infrastructure.

## 8. Rust Workspace Layout

Struktur workspace yang disarankan:

```text
sawit-engine/
  Cargo.toml
  crates/
    sawit_app/              # executable game client
    sawit_render/           # renderer abstraction
    sawit_input/            # keyboard/mouse/gamepad input
    sawit_world/            # world/chunk/block/entity state
    sawit_physics/          # movement/collision helpers
    sawit_net_client/       # client networking
    sawit_net_server/       # realtime game server
    sawit_protocol/         # shared packet/schema/types
    sawit_math/             # Vec3, Quat, transforms, fixed types
    sawit_assets/           # asset loading
    sawit_diagnostics/      # logs, telemetry, debug overlay
    sawit_lobby/            # HTTP lobby/matchmaking service
  tools/
    packet_inspector/
    load_tester/
    world_migrator/
```

### Kenapa multi-crate?

- Client dan server bisa share protocol tanpa circular dependency.
- Realtime server tidak perlu depend ke renderer.
- Load tester bisa pakai protocol yang sama.
- Build dan testing lebih bersih.
- Scaling organisasi kode jauh lebih enak.

## 9. Crate Responsibilities

### 9.1 `sawit_app`

Executable game client.

Tanggung jawab:

- Init window/render/input.
- Main loop.
- Integrasi local prediction.
- Integrasi network update.
- Render remote players dan world events.

### 9.2 `sawit_render`

Tanggung jawab:

- Abstraction renderer.
- Mesh/material/texture/shader pipeline.
- Camera.
- Debug draw.
- Remote player visualization.

Renderer tidak boleh depend ke networking.

### 9.3 `sawit_world`

Tanggung jawab:

- Entity state.
- Block/chunk state.
- World mutation API.
- Spatial query.
- Chunk subscription metadata.

### 9.4 `sawit_physics`

Tanggung jawab:

- Movement rules.
- Collision check.
- Grounding.
- Movement validation helper.

Rules di crate ini sebaiknya bisa dipakai client dan server agar prediction dan authority mirip.

### 9.5 `sawit_protocol`

Shared protocol crate.

Tanggung jawab:

- Packet enum.
- Versioning.
- Serialization/deserialization.
- Input command type.
- Snapshot type.
- World event type.
- Error codes.

Contoh tipe:

```rust
pub type PlayerId = u64;
pub type RoomId = u64;
pub type Tick = u32;

pub struct InputCommand {
    pub seq: u32,
    pub tick: Tick,
    pub dt_ms: u16,
    pub movement: MoveInput,
    pub yaw: f32,
    pub pitch: f32,
    pub action: Option<PlayerAction>,
}

pub struct PlayerSnapshot {
    pub player_id: PlayerId,
    pub position: Vec3,
    pub velocity: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub flags: PlayerFlags,
    pub last_processed_input: u32,
}

pub enum ClientPacket {
    Hello(ClientHello),
    Auth(AuthToken),
    Input(InputCommand),
    Action(PlayerAction),
    Ping(Ping),
    Disconnect,
}

pub enum ServerPacket {
    Welcome(Welcome),
    RoomState(RoomState),
    Snapshot(WorldSnapshot),
    WorldEvent(WorldEvent),
    Correction(PlayerCorrection),
    Pong(Pong),
    Error(ServerError),
}
```

### 9.6 `sawit_net_client`

Tanggung jawab:

- Connect/disconnect.
- Send input command.
- Packet receive queue.
- Snapshot buffer.
- Ping measurement.
- Packet loss estimate.
- Reconnect handling.

### 9.7 `sawit_net_server`

Tanggung jawab:

- Accept connection.
- Auth token validation.
- Assign player id.
- Room lifecycle.
- Simulation tick.
- Validate input.
- Broadcast snapshot.
- Interest management.
- Rate limit.

### 9.8 `sawit_lobby`

HTTP service.

Tanggung jawab:

- Create room.
- List room.
- Join room.
- Return realtime server address + short-lived token.
- Register active game servers.

## 10. Networking Stack Recommendation

### Option A — `renet` + UDP

Cocok untuk game networking Rust.

Kelebihan:

- Sudah game-oriented.
- Ada reliable/unreliable channel.
- Lebih cepat untuk prototype.

Kekurangan:

- Tetap perlu desain authority/snapshot sendiri.

### Option B — QUIC via `quinn`

Kelebihan:

- Encrypted by default.
- Connection-oriented.
- NAT/firewall lebih nyaman daripada raw UDP di beberapa kasus.
- Reliable streams + datagram support.

Kekurangan:

- Lebih kompleks.
- Datagram support dan tuning perlu hati-hati.

### Option C — Custom UDP

Kelebihan:

- Kontrol penuh.
- Bisa sangat optimal.

Kekurangan:

- Harus bikin reliability, ordering, ack, fragmentation, congestion behavior sendiri.
- Tidak disarankan untuk MVP kecuali tujuannya belajar low-level networking.

Rekomendasi MVP: `renet`.

Rekomendasi production-minded: mulai dari `renet`, desain protocol tetap bersih agar bisa pindah ke QUIC/custom transport tanpa rewrite gameplay.

## 11. Gameplay Replication Model

### 11.1 Local Player

Flow client:

```text
Read input
  -> Create InputCommand(seq)
  -> Apply local prediction immediately
  -> Send InputCommand to server
  -> Store command in pending input buffer
```

### 11.2 Server

Flow server:

```text
Receive InputCommand
  -> Validate sequence/rate
  -> Simulate authoritative movement
  -> Store player state
  -> Include state in next snapshot
```

### 11.3 Client Reconciliation

Flow saat snapshot/correction datang:

```text
Receive authoritative state
  -> Set local player to server state
  -> Drop acknowledged input commands
  -> Replay pending input commands
```

### 11.4 Remote Player

Remote player tidak perlu prediction penuh.

```text
Receive snapshots
  -> Store in interpolation buffer
  -> Render remote player slightly behind server time
  -> Interpolate position/rotation
```

## 12. Server Tick dan Rate

Rekomendasi awal:

- Server tick: 20 Hz.
- Snapshot send: 10–20 Hz.
- Client input send: 30–60 Hz.
- Render FPS: bebas.
- Interpolation delay: 100–150 ms.
- Max room MVP: 8–16 players.

Untuk scalable stage:

- Server tick tetap 20–30 Hz.
- Snapshot relevansi berdasarkan area/chunk.
- Packet budget per client.
- Compression/delta snapshot.

## 13. World Architecture

### 13.1 MVP World

Untuk awal:

- Room punya satu world in-memory.
- World mutation disimpan sebagai event list.
- Semua player dalam room menerima semua world event.

Cocok untuk small room.

### 13.2 Scalable World

Gunakan chunked world.

```text
ChunkCoord { x: i32, y: i32, z: i32 }
Chunk size: 16 x 16 x 16 atau 16 x 256 x 16
Chunk storage: sparse map / palette-compressed blocks
```

Server menyimpan:

```text
RoomState
  players: HashMap<PlayerId, PlayerState>
  chunks: HashMap<ChunkCoord, ChunkState>
  subscriptions: HashMap<PlayerId, HashSet<ChunkCoord>>
```

Client hanya receive:

- Player di radius relevan.
- Chunk dekat player.
- World event di subscribed chunks.

## 14. Interest Management

Interest management wajib untuk scalability.

Strategi awal:

```text
Interest radius = 2–4 chunk dari posisi player
```

Server per tick:

1. Hitung chunk player saat ini.
2. Update subscribed chunks.
3. Kirim chunk enter/exit jika berubah.
4. Kirim snapshot entity yang berada dalam interest set.
5. Kirim world event hanya untuk subscribed chunks.

## 15. Backend Scaling

Pola scalable yang dipakai untuk game real-time:

```text
Client
  ↓
Gateway / Load Balancer
  ↓
Matchmaking / Lobby
  ↓
Game Server Instance
  ↓
Database / Cache / Queue
```

Untuk game real-time, biasanya:

```text
1 room / match = 1 game server process / instance
```

Contoh mapping:

```text
Match A -> Game Server 1
Match B -> Game Server 2
Match C -> Game Server 3
```

Kalau player makin banyak, sistem tidak membuat satu server raksasa. Sistem akan spawn atau assign lebih banyak instance game server.

Komponen yang harus dipisah:

```text
Auth Server      -> login, token
Lobby Server     -> party, matchmaking, room discovery
Game Server      -> gameplay real-time
Chat Server      -> chat / presence
Inventory Server -> item, currency, ownership
Database         -> data permanen
Redis            -> session / cache / presence / room registry
Queue            -> async jobs / event processing
```

Untuk gameplay real-time:

- Game server jangan menjadi tempat utama data permanen.
- Anggap game server disposable.
- Setelah match selesai, game server mengirim hasil ke backend.
- Backend menyimpan hasil ke database.
- Kalau game server mati, match bisa disconnect, reconnect, rollback, atau dianggap selesai tergantung jenis game.

Pola scalable umum:

```text
1. Player login
2. Player masuk lobby
3. Matchmaker cari room / match
4. Orchestrator spawn atau pilih game server kosong
5. Lobby mengirim server address + join token ke client
6. Client connect langsung ke game server
7. Game server menjalankan gameplay real-time
8. Match selesai
9. Game server kirim result/event ke backend
10. Backend simpan data permanen
11. Game server instance dibersihkan atau dipakai ulang
```

### Stage 1 — Single Binary Dev

```text
sawit_net_server
  - room manager in-memory
  - no auth
  - direct connect
```

### Stage 2 — Lobby + Game Server

```text
sawit_lobby
  - HTTP API
  - create/list/join room

sawit_net_server
  - realtime gameplay
  - registers itself to lobby
```

### Stage 3 — Multi Server

```text
Load Balancer
  -> Lobby API
  -> Game Server Pool
  -> Redis Room Registry
  -> PostgreSQL Persistence
```

### Stage 4 — Multi Region

```text
Region Matchmaker
  -> assign nearest region
  -> allocate room to game server
  -> return server endpoint + token
```

## 16. Persistence Strategy

MVP:

- No persistence, or save room snapshot on shutdown.

Next:

- PostgreSQL for accounts/room metadata.
- Redis for active room registry.
- Object storage or DB blobs for world snapshot.
- Append-only event log for block mutations.

Scalable world:

- Save per chunk.
- Dirty chunk flush every N seconds.
- Snapshot + delta log.

## 17. Security dan Anti-Cheat Dasar

Server harus validate:

- Max speed per tick.
- Max acceleration.
- Jump only if grounded.
- Reach distance untuk interaction.
- Rate limit action.
- Packet size limit.
- Token validity.
- Sequence number monotonic.
- World mutation permission.

Client tidak boleh dipercaya untuk:

- Final position.
- Health/damage.
- Inventory.
- Block/world result.
- Spawn item.
- Teleport.

## 18. MVP Requirements

### Functional

1. Client bisa connect ke realtime server.
2. Server assign unique `PlayerId`.
3. Dua client bisa join room yang sama.
4. Client A melihat Client B bergerak.
5. Client B melihat Client A bergerak.
6. Local player tetap responsive melalui prediction.
7. Remote player smooth melalui interpolation.
8. Server bisa broadcast snapshot 10–20 Hz.
9. Player bisa melakukan action sederhana, misalnya interact/place/remove.
10. Server validate action lalu broadcast world event.
11. Disconnect tidak membuat room/client crash.
12. Debug overlay menampilkan ping, packet loss, snapshot rate.

### Non-Functional

- MVP room: 8–16 players.
- LAN latency block/world event <200 ms.
- Playable sampai 150 ms ping.
- Server tidak crash dari malformed packet.
- Semua packet parser punya size/bounds check.
- Server tick stabil 20 Hz untuk 16 player.

## 19. Protocol Versioning

Semua packet harus punya:

```text
protocol_version
packet_type
sequence/ack where needed
payload_len
payload
```

Untuk Rust-only, bisa pakai:

- `serde` + `bincode` untuk cepat.
- `bitcode` untuk binary efficient Rust ecosystem.
- `postcard` untuk compact binary.
- FlatBuffers/Cap’n Proto kalau butuh lintas bahasa nanti.

Rekomendasi awal: `serde` + `bincode`/`postcard`, tapi bungkus dengan `sawit_protocol` agar mudah diganti.

## 20. Suggested Crates

Client/game:

- `winit` untuk window/input.
- `wgpu` untuk renderer modern, atau tetap OpenGL wrapper jika mau lanjut gaya repo lama.
- `glam` untuk math.
- `tracing` untuk logging.

Networking:

- `renet` untuk realtime multiplayer transport.
- `tokio` untuk async services.
- `axum` untuk lobby API.
- `serde` untuk schema.
- `bincode` atau `postcard` untuk packet encoding.

Backend:

- `redis` untuk room registry.
- `sqlx` untuk PostgreSQL.
- `prometheus` atau OpenTelemetry untuk metrics.

## 21. Data Flow Join Room

```text
Client
  -> POST /rooms/{id}/join to Lobby
  <- receives server_addr + join_token

Client
  -> connect to Realtime Server
  -> send Hello + join_token

Realtime Server
  -> validate token with Lobby/Auth
  -> assign PlayerId
  -> send Welcome + initial RoomState

Client
  -> start sending InputCommand
  <- receive snapshots/world events
```

## 22. Roadmap Implementasi

### Milestone 1 — Rust Workspace Setup

- Buat Cargo workspace.
- Buat crates: `sawit_app`, `sawit_protocol`, `sawit_net_client`, `sawit_net_server`, `sawit_world`, `sawit_math`.
- Shared protocol compile di client dan server.

### Milestone 2 — Realtime Connection

- Server jalan local.
- Client connect/disconnect.
- Hello/Welcome.
- Ping/Pong.
- Debug latency.

### Milestone 3 — Player Replication

- InputCommand dari client.
- Authoritative PlayerState di server.
- Snapshot broadcast.
- Remote player interpolation.

### Milestone 4 — Prediction + Reconciliation

- Local prediction.
- Pending input buffer.
- Server correction.
- Replay unacknowledged inputs.

### Milestone 5 — World Event Replication

- Action command.
- Server validation.
- WorldEvent broadcast.
- Client apply event.

### Milestone 6 — Lobby Service

- `sawit_lobby` HTTP API.
- Create/list/join room.
- Join token.
- Game server registration.

### Milestone 7 — Scalable Room Server

- Multiple rooms per server.
- Room cleanup.
- Metrics.
- Load test tool.

### Milestone 8 — Chunked World + Interest Management

- Chunk storage.
- Subscription radius.
- Entity relevance.
- Snapshot delta.

## 23. Risiko Teknis

| Risiko | Dampak | Mitigasi |
|---|---:|---|
| Semua logic ditaruh di satu crate | High | Workspace multi-crate dari awal |
| Client terlalu dipercaya | High | Server authoritative |
| Snapshot terlalu besar | High | Delta + interest management |
| Movement terasa delay | High | Client prediction + reconciliation |
| Remote player jitter | Medium | Interpolation buffer |
| Custom UDP terlalu cepat dipilih | Medium | Mulai dari `renet` |
| World tidak chunked | Medium | MVP boleh simple, tapi desain API harus chunk-ready |
| Tidak ada load testing | Medium | Buat `tools/load_tester` sejak awal |

## 24. Acceptance Criteria MVP

MVP selesai kalau:

- Bisa run `sawit_net_server`.
- Bisa run dua instance `sawit_app`.
- Keduanya join room yang sama.
- Pergerakan player A muncul di player B.
- Pergerakan player B muncul di player A.
- Server authoritative terhadap posisi.
- Remote movement smooth tanpa teleport terus-menerus.
- Satu action world tersinkron ke semua client.
- Disconnect/reconnect aman.
- Ada debug ping dan snapshot rate.

## 25. Kesimpulan

Kalau project ini memang Rust, arsitektur terbaik adalah full Rust workspace dengan pemisahan jelas antara client, server, protocol, world, physics, dan lobby. Jangan bikin client dan server masing-masing punya definisi packet sendiri. Buat `sawit_protocol` sebagai kontrak bersama.

Untuk scalable path: mulai dari room-based authoritative server, lalu tambah lobby, room registry, persistence, chunked world, dan interest management. Dengan begitu MVP tetap cepat jadi, tapi fondasinya tidak mentok saat player bertambah.

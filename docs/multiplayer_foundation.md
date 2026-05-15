# Sawit Multiplayer: PRD & Scalable Architecture

> Sumber-kebenaran: dokumen ini; ringkasan repo ada di `README.md`.
> Terakhir diperbarui: 2026-05-15 (Asia/Jakarta).
> Encoding: UTF-8.

## 1. Ringkasan

Sawit Engine diposisikan sebagai game/engine berbasis Rust dengan arsitektur multiplayer scalable. Fokus dokumen ini adalah menyatukan visi produk (PRD) dengan rancangan teknis (Arsitektur) agar player bisa saling terhubung, sinkron, dan sistem bisa berkembang dari prototype hingga production.

Pendekatan utama:
- **Server Authoritative**: Server adalah sumber kebenaran.
- **TCP/UDP Split**: Memisahkan jalur administratif (TCP) dan jalur gameplay realtime (UDP).
- **Shared Protocol**: Definisi data yang sama antara client dan server.
- **Scalable Design**: Menggunakan room-based session yang stateful namun bisa di-scale horizontal.

---

## 2. Masalah & Tujuan

### Masalah yang Diselesaikan
1. **Desync**: Player antar client sering beda posisi/state.
2. **Cheating**: Client-side authority sangat rawan dimanipulasi.
3. **Complexity**: P2P ribet dengan NAT traversal dan sinkronisasi state banyak orang.
4. **Scalability**: Mengirim semua state ke semua orang tidak efisien.

### Tujuan Utama
- Player bisa connect, join room, dan melihat player lain secara real-time.
- Movement tetap smooth meskipun ada latency (Prediction + Interpolation).
- Aksi dunia (place/remove/interact) tersinkron dengan benar.
- Arsitektur siap dipecah menjadi microservices jika beban meningkat.

---

## 3. Prinsip Arsitektur

1. **Server Authoritative**: Server menentukan posisi final, damage, dan world mutation.
2. **Client Responsive**: Client melakukan local prediction agar input terasa instan.
3. **Snapshot + Interpolation**: Remote player digerakkan dari buffer snapshot yang diinterpolasi.
4. **Room/Session Isolation**: Satu room/world bisa dipindahkan ke process/server berbeda tanpa mengganggu yang lain.
5. **Interest Management**: Server hanya mengirim data yang relevan (radius/chunk) ke player.
6. **Async-first (Rust)**: Menggunakan async runtime untuk handling koneksi masif, namun simulation tick tetap deterministic.

---

## 4. Analogi & Pembagian Jalur (TCP/UDP)

Bayangkan game server seperti kota multiplayer:
- **TCP (Administrasi)**: Kantor pendaftaran. Mengurus hal yang perlu rapi, reliable, dan terurut.
- **UDP (Jalan Raya)**: Jalur cepat realtime. Mengurus hal yang perlu instan dan boleh melepas data lama.

### 4.1 TCP Control Plane (Reliable)
Digunakan untuk: Login/Auth, Create/Join Room, Chat/Lobby, Inventory, Discovery.
- **Analogi**: Resepsionis. Player datang, cek ID, diberi nomor room dan alamat UDP, lalu diarahkan masuk arena.
- **Penting**: TCP tidak menjalankan movement tick agar tidak terkena *head-of-line blocking*.

### 4.2 UDP Realtime Data Plane (Unreliable)
Digunakan untuk: Input movement, Camera yaw/pitch, Realtime actions, World snapshots, Correction.
- **Analogi**: Jalur express di arena. Input dilempar cepat, snapshot dikirim balik berkala. Data lama dibuang jika data baru sudah datang.

---

## 5. Rancangan Sistem & Flow

### 5.1 High-Level Diagram
```text
+-------------------------+          +-------------------------+
|    Rust Game Client     |          |    sawit-service.rs     |
|-------------------------|          |-------------------------|
| - Renderer & Input      |  (TCP)   | - TCP Gateway           |
| - Local Prediction      | <------> | - Session & Room Mgr    |
| - Interpolation Buffer  |          | - Tick Scheduler        |
| - TCP/UDP Net Client    |  (UDP)   | - Snapshot Replication  |
+-------------------------+ <------> +-------------------------+
                                              |
                                     +-------------------------+
                                     | Persistence (Redis/PG)  |
                                     +-------------------------+
```

### 5.2 Flow Join Player
1. **Client -> TCP**: Request "Join Room 42".
2. **TCP Gateway**: Validasi token, pilih Room Worker yang tersedia.
3. **TCP -> Client**: Kirim `udp_addr`, `room_id`, dan `join_token`.
4. **Client -> UDP**: Kirim `Hello(token)`.
5. **UDP Server**: Bind address ke player ID.
6. **Gameplay Start**: Client kirim input, Server balas snapshot.

---

## 6. Rust Workspace & Crate Structure

Agar pengembangan bersih dan scalable, project dibagi menjadi beberapa crate:

- `sawit_app`: Executable game client (Window, Render, Input loop).
- `sawit_world`: State management untuk entity, chunk, dan world mutation.
- `sawit_physics`: Rules pergerakan dan collision (dipakai Client & Server).
- `sawit_protocol`: **Shared Crate**. Definisi packet, schema, dan serialization (Postcard).
- `sawit_net_client`: Wrapper networking sisi client.
- `sawit_net_server`: Core logic game server (Room registry, Tick loop, Broadcast).
- `sawit_math`: Shared math types (Vec3, Quat, Transform).

---

## 7. Model Replikasi & Sinkronisasi

### 7.1 Local Player (Prediction)
- Client baca input -> Apply lokal langsung -> Kirim ke Server.
- Client simpan input di buffer pending.

### 7.2 Server (Authority)
- Server terima input -> Validasi -> Jalankan simulasi authoritative.
- Server kirim snapshot posisi resmi balik ke client.

### 7.3 Reconciliation
- Client terima snapshot -> Set posisi ke posisi server.
- Client *replay* input dari buffer yang belum di-acknowledge server agar posisi kembali ke masa kini.

### 7.4 Remote Players (Interpolation)
- Client terima snapshot player lain -> Simpan di buffer.
- Render player lain sedikit terlambat (mis. 100ms) dengan menginterpolasi antar dua snapshot terakhir agar gerakan terlihat smooth meskipun network jitter.

---

## 8. Scaling Roadmap

### Stage 1: Single Binary (Current)
- UDP & TCP server dalam satu process.
- Room map in-memory.
- Fokus: Stabilitas local multiplayer dan integrasi engine.

### Stage 2: Distributed Lobby
- Pisahkan `sawit_lobby` (TCP/HTTP) sebagai entry point utama.
- Game Server (UDP) hanya fokus ke simulasi.

### Stage 3: Multi Game Server (Orchestration)
- Menggunakan **Room Directory** (Redis).
- Lobby bisa mengarahkan player ke Server A atau Server B tergantung load room.

### Stage 4: Interest Management (Deep Scalability)
- Server tidak lagi mengirim "semua" entity ke "semua" player.
- Snapshot difilter berdasarkan radius/chunk di sekitar player.

---

## 9. Pemetaan ke Kode (Mapping to Code)

Arsitektur ini sudah diimplementasikan di `sawit-service`:

- `src/server.rs`: Orchestrator utama (TCP + UDP Gateway).
- `src/tcp.rs`: Control plane (JOIN, ROOMS, HEALTH).
- `src/udp.rs`: Data plane (Input processing, Snapshot fanout).
- `src/room.rs`: Stateful room/player registry & snapshot builder.
- `src/simulation.rs`: Logika pergerakan authoritative.
- `src/wire.rs`: Encoder/Decoder (Postcard & Text protocol untuk C engine).
- `src/protocol.rs`: Kontrak data/packet.

---

## 10. Konfigurasi Standar (Default)

- **Server Tick**: 20 Hz (setiap 50ms).
- **Snapshot Rate**: 20 Hz.
- **Client Input**: 30-60 Hz.
- **Interpolation Delay**: ~100 ms.
- **Max Player/Room (MVP)**: 8–16 players.

---
title: Simulating 2G to build an offline-first sync engine in Rust for rural clinics
published: false
description: I built a lightweight sync engine in Rust for clinics with bad network, and tested it using Toxiproxy to simulate 2G drops.
tags: rust, architecture, networking, showdev
---

hey dev.to,

so I am Mathéo, a 2nd year CS student at EPITECH Nancy (France). I want to share a project I have been building for the last few months: **ZamSync**. 

Basically, it is a lightweight synchronization engine written in Rust. It is made for places where the internet is either extremely slow, drops every five minutes, or does not exist at all (think remote areas, Raspberry Pi nodes, offline-first apps).

The exact scenario I had in mind is the electronic Patient Information System (ePIS) of Bhutan. In rural Bhutan, district clinics need to register patients and sync the data back to central hospital hubs. The problem is the network: they often have 2G connections, 600ms latency, and constant cuts. 

I looked at existing tools, but they either require a permanent stable connection or need a heavy database engine running on the client. I wanted to build something that:
1. Works over days of total disconnection.
2. Recover instantly if the network drops mid-sync (without duplicating data).
3. Runs on tiny hardware (under 10MB of RAM on a Raspberry Pi).
4. Keeps everything secure (encrypted at rest and in transit).

Here is the GitHub repository if you want to inspect the code:
https://github.com/Etoile-Bleu/ZamSync

---

## How it works under the hood

The engine is built around a simple principle: append-only logs. 

Instead of syncing a database state directly, ZamSync syncs **events** (which can carry JSON payloads, like patient check-ins). 

Here is the technical stack:

* **Write-Ahead Log (WAL):** Events are written locally to an encrypted, append-only file. Each record has a CRC32 integrity check to prevent corruption.
* **Hybrid Logical Clocks (HLC):** Since clinics do not have reliable NTP servers to sync their system clocks, we use HLCs to order events deterministically across different nodes.
* **Version Vectors:** When two nodes connect, they exchange their version vectors (essentially a map of who has seen what sequence number). They compare them, find the exact gaps, and only send the missing events.
* **WAL Encryption:** The log file is encrypted at rest using ChaCha20-Poly1305. A random 96-bit nonce is generated for every single record, and keys can be rotated.
* **mTLS (mutual TLS):** The network layer uses custom mTLS certificates signed by the hub CA. If a node does not have a valid signed certificate, it is rejected during the TLS handshake before any data can be read.

I also implemented a `--policy own` access control. If Clinic A and Clinic B both sync to the same Hospital Hub, Clinic A cannot download the events submitted by Clinic B. The hub keeps everything, but restricts sync replies based on the client certificate identity.

---

## Simulating the 2G network with Toxiproxy

To verify if this actually works in real-world conditions, I did not want to just write standard unit tests on localhost. 

I set up a test suite using Docker Compose and **Toxiproxy** (an awesome tool by Shopify to simulate network failure). The test setup does this:
1. Starts a hub node and a client node.
2. Interposes Toxiproxy between them.
3. Sets the link to emulate a bad rural 2G network: 600ms latency, 100ms jitter, 30 KB/s bandwidth cap.
4. Generates 5,000 events.
5. In the middle of the transfer, the test script cuts the connection completely for a few seconds.
6. Reconnects and runs the sync again.

In the end, all 5,000 events are replicated with zero loss, zero duplicates, and no corrupted states.

If you have Docker installed, you can run this test yourself:
```bash
docker compose -f tests/docker-compose.test.yml up --build --abort-on-container-exit
```

---

## The codebase structure

The project uses a clean Rust workspace split into 4 crates:
* `zamsync-core`: Pure state logic, events, HLCs, and ports. No I/O.
* `zamsync-storage`: The engine implementation and the WAL filesystem code.
* `zamsync-network`: The network layer, TCP transport, frame format, and mTLS logic.
* `zamsync-testing`: Helper tools and integration tests.

I also cross-compile static musl binaries for `x86_64`, `aarch64` (Raspberry Pi 4), and `armv7` (Raspberry Pi 3) so that installing it on remote Linux machines is just a curl command with no library dependencies.

---

## Next steps & feedback

This is my first time building a low-level sync engine in Rust, and I would love to get feedback from other systems engineers. 

Does the WAL approach make sense for this use case? Are there edge cases in my Version Vector implementation that I might have missed?

If you want to check the architecture or play with the CLI, here is the link:
[GitHub - Etoile-Bleu/ZamSync](https://github.com/Etoile-Bleu/ZamSync)

Thanks for reading!
- Mathéo

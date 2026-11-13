# link_lab — design and work plan

**Status:** accepted (L1–L12 accepted as recommended by the maintainer).
**Scope:** `link_lab`, a cFS application written in Rust that carries Space Packets over CCSDS TM and TC frames with COP-1, using [Aldis](https://github.com/kscharpf/aldis); and the smallest cFS flight node around it that can be checked against the Aldis reference-application golden traces.

## Summary

| Item | Value | Section |
|---|---|---|
| What it is | A cFS app that replaces CI_LAB and TO_LAB with a real data link layer: TM frames down, TC frames with FARM-1 up | §1 |
| Why | cFS has no data link layer; Aldis's value is its link layer. This is where the two projects meet, and it is the "new apps in Rust on the C core" migration path | §1 |
| Specification and oracle | The Aldis reference application: same scenario, same golden traces, byte for byte | §2, §4.6 |
| New crates | `cfe-sys` (raw cFE and OSAL declarations), `cfe` (safe wrappers), `link-lab-core` (sans-I/O link logic), `link-lab` (cFS glue) | §4.2 |
| Other cFS apps needed | `step_lab` (bridge to the harness, sequencer) and `scn_lab` (the scenario's flight applications) | §4.1 |
| Determinism | Lockstep: the harness is the clock, one app runs at a time, scenario time comes from the wake message | §4.5 |
| Needs from Aldis | The out-of-process node protocol over TCP in the harness; a pinned revision that contains the A1–A3 golden traces; a public header-only packet decode | §3.3 |
| Phases | Bindings → bridge → packets (`a1-*`) → frames (`a2-*`) → COP-1 (`a3-*`) → exit | §7 |

## 1. Purpose

The project goals this serves:

| Goal | How link_lab serves it |
|---|---|
| Learn cFS | The bindings and three apps exercise the Software Bus, the executive (app lifecycle, startup sync), event services, mission configuration, and the app build, which the MSG port does not touch |
| Value to Aldis | A second, independent consumer of the Aldis link engines and COP-1, inside a flight framework, checked against the same golden traces as Aldis's own flight node |
| High-assurance technique | The sans-I/O core / thin glue split, the flight lint set, panic freedom and no heap, carried from a library into an application on an RTOS-style framework |
| Migration pathway | A cFS app in Rust on the unmodified C cFE, built by the cFS CMake build and loaded by the stock startup script |

## 2. Relationship to the Aldis reference application and to ACFS

| | Aldis Rust flight node | ACFS (Aldis plan, on hold) | This design |
|---|---|---|---|
| Runs on cFS | No | Yes | Yes |
| App language | Rust | C, through `aldis.h` | Rust, Aldis crates directly |
| Lives in | Aldis `refapp/` | Aldis `refapp/cfs/` | cfs-rs |
| Link code | In-process, one node struct | `refapp_ci` and `refapp_to` | One app, `link_lab` |
| Oracle | Golden traces | Same traces | Same traces |

This design reuses two accepted ACFS decisions unchanged, because they are about cFS and not about the language: lockstep sequencing (ACFS D8) and the node protocol over loopback TCP through OSAL sockets (ACFS D9). It differs from ACFS in three ways:

- **One link app, not two.** FARM-1's CLCW goes into the OCF of every TM frame. With uplink and downlink in one app this is a function call; split across CI and TO it is a Software Bus message on the critical path of every frame.
- **The scenario app builds the packets.** ACFS has the telemetry-output app build each Space Packet. Here `link_lab` receives finished packets, as a link layer does, so it stays reusable outside the scenario.
- **Per-packet uplink hand-off** (§4.5), which ACFS does not specify.

## 3. Starting point

### 3.1 In cfs-rs

| In place | Missing |
|---|---|
| cFS v7.0.1 as an unmodified submodule; the `CFS_APP_PATH` override; a container that builds cFS and runs its tests | Any cFS app; a mission defs directory of our own (`targets.cmake`, startup script) |
| A Rust static library built by Cargo from inside the cFS CMake build (`modules/msg/CMakeLists.txt`) | The same for a loadable app module |
| `cfe-msg`, `cfe-msg-ffi`: the Rust MSG module | Rust bindings to any cFE or OSAL API |

### 3.2 In Aldis

Read from the local Aldis checkout at `4e4468f9` (812 commits after `v0.6.0-m5`, the newest tag; 21 behind `origin/main`). cfs-rs has had no Aldis dependency since #10, so this plan chooses the pin afresh (L9).

| In place | Missing |
|---|---|
| Scenario A1 (packets), A2 (frames), A3 (COP-1) exited; 14 golden traces `a0-empty`, `a1-*`, `a2-*`, `a3-*` in `refapp/golden/` | A release tag that contains them (the newest tag is still `v0.6.0-m5`) |
| `refapp-flight`: the Rust flight node, `no_std`, no heap, with public modules `command`, `downlink`, `dump`, `event`, `layout`, `queue`, `schedule`, `table`, `frames_down`, `frames_up` | Its per-step phases (`handle_delivered`, `execute_due`, `dump_segments`) are `pub(crate)` |
| `refapp-harness`: scheduler, link model, faults, trace writer, comparator, in-process nodes | The out-of-process node protocol (reference-application §8.2) and its TCP transport: I found no implementation in `refapp-harness/src` |
| `refapp-scenario`: generated scenario constants and the `Node`, `Output`, `StepInput` types | — |

### 3.3 What this plan needs from Aldis

| # | Need | Why | Size |
|---|---|---|---|
| [A-1](https://github.com/kscharpf/aldis/issues/588) | The node protocol of reference-application §8.2 in the harness, over TCP, with a launcher that can start an external `core-cpu1` (ACFS WP-2) | Without it no cFS node can be checked against a trace | M |
| [A-2](https://github.com/kscharpf/aldis/issues/589) | A tag or agreed revision that contains `refapp/` at A3 and the golden traces | The newest tag predates the traces, so there is nothing stable to pin (L9) | S |
| [A-3](https://github.com/kscharpf/aldis/issues/590) | The packet-level phases of `FlightNode` made public, or an equivalent entry point per phase | `scn_lab` must run the scenario's steps one at a time (§4.5). Without it, `scn_lab` re-implements about 230 lines of `handler.rs` sequencing | S |
| [A-4](https://github.com/kscharpf/aldis/issues/591) | `SpacePacketHeader` decode made public | Lets `link_lab` route on APID from six octets. Not blocking: `SpacePacket::decode` works because packets arrive whole | S |

A-1 is the critical path. It is the same work whether ACFS or this plan uses it first.

### 3.4 cFS facts this design relies on

Read from the submodule at v7.0.1. ACFS facts F1–F18 (Aldis `docs/milestones/acfs-flight-side.md` §1.1) also apply and are cited as F*n*.

| # | Fact | Source |
|---|---|---|
| C1 | An app is built with `add_cfe_app(<name> <sources>)` and started by a `CFE_APP, <file>, <entry>, <name>, <priority>, <stack>, 0x0, 0;` line in the startup script. The sample script gives each app a 16384-octet stack | `apps/sample_app/CMakeLists.txt`; `cfe/cmake/sample_defs/cpu1_cfe_es_startup.scr` |
| C2 | `CFE_SB_TransmitMsg(const CFE_MSG_Message_t *MsgPtr, bool IsOrigination)`: the caller chooses whether cFE applies origination actions to the header | `cfe/modules/core_api/fsw/inc/cfe_sb.h:438` |
| C3 | Platform message IDs are `base | topic`, with telemetry base `0x0800` and command base `0x1800`; global topics use `0x0860` and `0x1860` | `cfe/modules/core_api/config/default_cfe_core_api_msgid_mapping.h:49–86` |
| C4 | The example configuration limits a pipe to 4 queued messages per message ID (`CFE_PLATFORM_SB_DEFAULT_MSG_LIMIT`) and a message to 32768 octets (`CFE_MISSION_SB_MAX_SB_MSG_SIZE`) | `cfe/cmake/sample_defs/example_platform_cfg.h:1098`, `example_mission_cfg.h:339` |
| C5 | OSAL has stream sockets: `OS_SocketOpen`, `OS_SocketConnect`, `OS_SocketRecvFrom`, `OS_SocketSendTo`, `OS_SocketAddrInit`, `OS_SocketAddrFromString`, `OS_SocketAddrSetPort` | `osal/src/os/inc/osapi-sockets.h:181–577` |

Not verified, and treated as risks in §9: how two Rust static libraries behave when loaded into one process by `dlopen` with `RTLD_GLOBAL` (F17), and whether the build image's `fs.mqueue.msg_max` is large enough for the pipe depths (F16).

## 4. Design

### 4.1 Process view

```
harness (Aldis refapp-harness, std)        cFS process core-cpu1 (pc-linux PSP, SIMULATION=native)
┌──────────────────────────────┐
│ clock, link model, faults,   │ node protocol   step_lab   bridge and sequencer
│ trace writer, comparator;    │◀─── TCP ──────▶   │
│ Rust ground node, in-process │  127.0.0.1        │ wake            wake
└──────────────────────────────┘                   ├───────────────▶ scn_lab   scenario flight applications
                                                   │                   │  ▲
                                                   │ wake    packets,  │  │ one command
                                                   ▼         records   ▼  │ packet at a time
                                                 link_lab ◀────────────┘  │
                                                   │  └───────────────────┘
                                                   │ units (frames), records, done
                                                   └──▶ step_lab ──▶ STEP_RESULT

                                   cFE core services as shipped; MSG from cfe-msg-ffi in the `rs` build
```

| cFS module | Replaces | Role | Aldis use |
|---|---|---|---|
| `step_lab` | SCH_LAB | Holds the harness connection. Turns each STEP into wake messages, one app at a time. Collects outputs and next deadlines, answers STEP_RESULT | None |
| `link_lab` | CI_LAB, TO_LAB | The data link layer. Downlink: packets into virtual-channel generators, multiplexer, one TM frame per transmit slot, CLCW in the OCF. Uplink: TC frame reception, FARM-1, MAP demultiplexing, segment reassembly, one command packet at a time to the Software Bus | `spp`, `tm`, `tc`, `link`, `cop1`, `core` |
| `scn_lab` | — | The scenario's flight applications: on-board time, housekeeping, events, payload, command handler with the time-tag queue, table store, memory dump. Built on the public modules of Aldis `refapp-flight` | `spp`, `time`, through `refapp-flight` |

`scn_lab` is one app in the first version. It exists so `link_lab` has something to carry; it is not the subject of this design (decision L5).

### 4.2 Crates and directories

| Path | Kind | Rules | Content |
|---|---|---|---|
| `crates/cfe-sys` | `no_std` library | Declarations only; `unsafe` allowed | `extern "C"` declarations and `#[repr(C)]` types for the cFE and OSAL functions used. Generated by `bindgen` in the container from the configured mission headers, committed, and checked current by `make node` (decision L3) |
| `crates/cfe` | `no_std` library | All `unsafe` of the app side lives here, each block with `// SAFETY:` | Safe wrappers: `es` (run loop, startup sync, exit), `evs` (register, send), `sb` (`Pipe`, `MsgId`, subscribe with a message limit, receive, transmit), `msg` (init, size), `osal::TcpStream` |
| `apps/link_lab/core` (`link-lab-core`) | `no_std` library | `#![forbid(unsafe_code)]`, flight lint set, no heap, no dependency on `cfe` | The sans-I/O link logic (§4.3). Depends on `aldis` only |
| `apps/link_lab` (`link-lab`) | `staticlib` | `#![deny(unsafe_code)]` with one `#[expect]` on the exported entry point | cFS glue: entry point `LINK_LAB_AppMain`, pipe, message encode and decode, calls into the core |
| `apps/step_lab`, `apps/scn_lab` | `staticlib` each, with a core crate where there is logic to test | As `link_lab` | §4.1 |
| `apps/*/CMakeLists.txt` | CMake | — | `add_cfe_app` with the Cargo-built static library, following `modules/msg/CMakeLists.txt` |
| `node_defs/` | cFS mission defs | — | `targets.cmake` (app list), platform and mission configuration, startup script, message ID allocation |

cfs-rs has no Aldis dependency today (removed from the MSG crates in #10). This plan adds it back for `link-lab-core` and `scn_lab` only. `cfe-sys`, `cfe`, `step_lab`, and the MSG crates stay free of it.

### 4.3 link-lab-core

The core is a state machine with no cFE, OSAL, or clock access, so it is unit-tested on the host by `make check`.

```rust
pub struct Link { /* generators, mux, FARM-1, reassemblers, counters */ }

pub enum Level { Packets, Frames, Cop1 }

impl Link {
    pub fn new(level: Level, config: &LinkConfig) -> Result<Link>;

    /// One delivered link unit. Calls `sink.command_packet` once per packet extracted, in order.
    pub fn accept_unit(&mut self, now: Instant, unit: &[u8], sink: &mut dyn UplinkSink);

    /// One telemetry packet from an application.
    pub fn accept_packet(&mut self, now: Instant, packet: &[u8], sink: &mut dyn DownlinkSink);

    /// The transmit slot. Emits exactly one unit at frame level when `tx_slot` is true.
    pub fn slot(&mut self, now: Instant, tx_slot: bool, sink: &mut dyn DownlinkSink);

    pub fn next_deadline(&self, now: Instant) -> Option<Instant>;
}
```

| Level | Downlink | Uplink | Reproduces |
|---|---|---|---|
| `Packets` | Each packet is validated with `SpacePacket::decode` and sent as one unit | Each unit is decoded as one Space Packet and passed on | `a1-*` |
| `Frames` | APID → virtual channel; one `aldis::link::generator` per VC with its release deadline and idle fill; `aldis::link::mux` with fixed priority; OID frame on VC 7 when nothing is released; OCF from the CLCW; FECF | `aldis::link::receiver` with the TC validity checks, Type-BD only, FARM-B counter in the CLCW; MAP 0 blocking, MAP 1 segment reassembly (`aldis::link::reassembler`) | `a2-*` |
| `Cop1` | As `Frames` | As `Frames`, with `aldis::cop1::farm1` seeing Type-AD and Type-BC frames | `a3-*` |

- **Configuration.** `LinkConfig` holds the managed parameters of reference-application §4 (frame lengths, spacecraft ID, VC and MAP sets, deadlines, window widths) and the APID → VC routing. In the first version it is built from `refapp-scenario`'s generated constants. A cFE table is a later option (§8).
- **Memory.** All buffers are fields of `Link`, sized by constants: no heap, no statics. The app holds its `Link` on the task stack, which the startup script must size for it (C1); WP-4 measures the size and sets the stack.
- **Own link code.** The core uses the Aldis engines directly and does not reuse `refapp-flight`'s `frames_down` and `frames_up` (decision L4). It will resemble them. The point is a second consumer whose inputs have a different shape: packets arriving one at a time as messages, not from a node struct.

### 4.4 Software Bus interface

Scenario packets travel on the bus wrapped as the payload of internal messages, not as native bus messages (decision L6):

- **Collision.** With message ID version 1 the ID is the first two octets of the primary header (F11). The scenario's housekeeping packet (APID `0x010`, telemetry, secondary header) would be `0x0810`, inside the range cFE allocates to its own platform telemetry (C3).
- **Header ownership.** The scenario fixes its own secondary headers, including a 7-octet form cFE does not have. Wrapped, no cFE header function ever touches scenario octets.

| Message | From → to | Payload | Purpose |
|---|---|---|---|
| `WAKE_RX` | `step_lab` → `link_lab` | `now`, one link unit | Process one delivered unit |
| `WAKE_TX` | `step_lab` → `link_lab` | `now`, `tx_slot` | Drain the pipe, then the frame of the slot |
| `WAKE_APPS` | `step_lab` → `scn_lab` | `now` | Scenario steps 2–5 (time-tagged commands, dump, housekeeping, payload) |
| `CMD_PACKET` | `link_lab` → `scn_lab` | `now`, one command or table-upload packet | Scenario step 1, one packet |
| `CMD_PACKET_DONE` | `scn_lab` → `link_lab` | — | Returns the turn |
| `TLM_PACKET` | `scn_lab` → `link_lab` | One telemetry packet | To be carried on the downlink |
| `RECORD` | `scn_lab` → `link_lab` → `step_lab` | An `app_event` record | For the trace, in order |
| `UNIT` | `link_lab` → `step_lab` | One link unit | A `unit` output |
| `DONE` | each app → `step_lab` (`scn_lab`'s through `link_lab`) | Next deadline | Ends the app's turn |
| `START` | `step_lab` → all | Link level of the case | Sent once after HELLO |

- **Message IDs** are allocated in `node_defs/` from topic IDs that cFE does not use; WP-1 fixes the values and adds a build-time check against the cFE ranges of C3.
- **Message limits.** Subscriptions set an explicit per-ID limit, because the default of 4 (C4) is below a step's burst of telemetry packets.
- **cFE's own traffic.** cFE housekeeping and EVS events stay outside the trace. Apps use EVS for diagnostics only.

### 4.5 Sequencing and record order

The golden trace fixes the order of every record. The Rust flight node produces them in one function, in this order: delivered units (each unit's packets handled as they are extracted), time-tagged commands, dump segment, housekeeping, payload, then the frame of the slot. The cFS node keeps that order with three rules, the first two from ACFS §10.5:

| Rule | Detail |
|---|---|
| One active app | Exactly one app runs at a time. `step_lab` wakes the next only after the previous `DONE` has come back |
| One ordered output stream | Every record and telemetry packet of `scn_lab` goes to `link_lab`'s single pipe; `link_lab` forwards records and emits its own in arrival order. A pipe is first in, first out, so the trace order is the production order |
| One command packet at a time | `link_lab` sends one `CMD_PACKET` and processes its pipe until `CMD_PACKET_DONE` before it extracts the next packet or raises the next link event |

The third rule is new. A TC frame on MAP 0 can carry several blocked commands, and an immediate command produces an event packet when it executes. Handing all packets over at once would put the link's later records ahead of the first command's records, which the Rust node does not do.

One STEP then runs as:

1. For each delivered unit: `WAKE_RX` to `link_lab`, which exchanges `CMD_PACKET` / `CMD_PACKET_DONE` with `scn_lab` per packet, then `DONE`.
2. `WAKE_APPS` to `scn_lab`; its packets and records queue in `link_lab`'s pipe, its `DONE` last.
3. `WAKE_TX` to `link_lab`: it drains the pipe in order, pushing each packet into the link, then emits the unit of the slot and `DONE`.
4. `step_lab` answers STEP_RESULT with the outputs in arrival order and the earliest deadline reported.

**Time.** Every wake carries `now`. No scenario value is read from cFE TIME or an OSAL or PSP clock; cFE TIME runs as shipped and outside the trace. Apps pend on their pipes without a timeout.

### 4.6 Verification

| Level | What | Where it runs |
|---|---|---|
| Unit | `link-lab-core` against hand-built packets and frames; property tests on packet → frame → packet round trips through an Aldis receiver | Host, `make check` |
| Differential | `link-lab-core` driven by the unit sequence of each golden trace, compared with the trace's flight-side records, without cFS | Host, `make check` |
| System | The cFS node under the Aldis harness: every `a1-*`, `a2-*`, `a3-*` case, byte equality with the golden trace, each case run twice | Container, new target `make node` |
| MSG matrix | The system level on both the stock MSG module and the Rust one | Container |
| Static | Flight lint set; no panic symbols in the app libraries; no heap symbols; stack use measured | Container |

The differential level is the one to build first. It checks the link logic against the oracle with no cFS in the loop, so a system-level mismatch can be attributed to the glue or the sequencing.

### 4.7 Build and link

- Each app's `CMakeLists.txt` runs `cargo build` for its `staticlib` and passes it to `add_cfe_app`, with the entry point forced into the link, as `modules/msg` does today.
- `cfe-sys` is generated from headers that only exist after the cFS build is configured, so it cannot be generated by `make check` on the host. The generated file is committed for the `node_defs` configuration and `make node` regenerates and compares it.
- Each app library exports its entry point only. All other symbols are hidden, so three Rust static libraries and `cfe-msg-ffi` do not see each other's symbols under `RTLD_GLOBAL` (F17). WP-1 proves this before any app has logic.

## 5. Non-goals of the first version

| Not included | Why |
|---|---|
| Native cFE messages on the link (cFE housekeeping and EVS in TM frames) | Needs a real-time mode outside the lockstep harness; listed in §8 |
| Real-time operation with SCH_LAB and a socket or radio | No golden trace covers it |
| AOS and USLP | The scenario's `a4-*` variants; add once the TM/TC path is done |
| Routing from a cFE table | Constants first; TBL is a good cFS exercise for later |
| A target other than pc-linux | cfs-rs cross builds are not set up |
| Changes to cFE, OSAL, PSP | The project rule |
| Synchronization and channel coding, SDLS | Not in the scenario |

## 6. Decisions for the maintainer

| # | Decision | Recommendation | Alternatives |
|---|---|---|---|
| L1 | Where the app lives | In cfs-rs under `apps/`, structured so it can move to its own repository later | A separate repository now (cleaner, but every binding change becomes a cross-repository change while the bindings are young) |
| L2 | One link app or two | One, `link_lab` | `tm_lab` and `tc_lab`, mirroring TO and CI (the CLCW then crosses the bus for every frame) |
| L3 | How `cfe-sys` is produced | `bindgen` CLI in the container, output committed and checked current | `bindgen` in `build.rs` (host `make check` cannot configure cFS); hand-written declarations (drift from the headers, the gap the architecture review found in `cfe-msg-ffi`) |
| L4 | Link code | Written in `link-lab-core` against the Aldis engines | Reuse `refapp-flight`'s `frames_down` and `frames_up` (less work, no second consumer of the engines) |
| L5 | Scenario applications | One app, `scn_lab`, on `refapp-flight`'s public modules | Three apps as ACFS D7 (more bus traffic to sequence, no more link evidence); a private re-implementation (duplicates scenario logic) |
| L6 | Scenario packets on the bus | Wrapped in internal messages | Native bus messages (message ID collision and header ownership, §4.4) |
| L7 | Determinism | Lockstep, as ACFS D8 | Real time with masked comparison (cannot reproduce the fault cases) |
| L8 | Transport | Node protocol over loopback TCP through OSAL, as ACFS D9 | stdin/stdout (the OSAL console owns stdout, F15) |
| L9 | Aldis pin | One revision for the library, `refapp-flight`, `refapp-scenario`, the harness, and the traces, moved to a tag once A-2 exists | Separate pins (the traces and the library that produced them could disagree) |
| L10 | ACFS | Stays on hold; A-1 is done once and serves both | Do ACFS's packet phase first to solve the cFS time and lockstep questions in C (less risk per step, more total work) |
| L11 | New dependencies and tools | `bindgen` and `libclang` in the container; `aldis` as a workspace dependency again, with features `core`, `spp`, `tm`, `tc`, `link`, `cop1`, `time`, `spp-time`; git dependencies on `refapp-flight` and `refapp-scenario` | — |
| L12 | Names | `link_lab`, `step_lab`, `scn_lab`; crate `cfe` for the safe wrappers | `tmtc_lab`; `cfe-rs` |

## 7. Work packages

Size is relative: **S**, **M**, **L**. "Aldis" marks work in the Aldis repository.

```
WP-0 decisions ─┬─▶ WP-1 bindings and skeleton ─┬─▶ WP-3 step_lab ─┐
                ├─▶ WP-2 harness (Aldis, A-1) ──┘                  ├─▶ WP-5 packets ─▶ WP-6 frames ─▶ WP-7 COP-1 ─▶ WP-8 exit
                └─▶ WP-4 link-lab-core (host only) ────────────────┘
```

### WP-0 Decisions (S)

L1–L12 accepted or changed. A-1 to A-3 raised in Aldis. `AGENTS.md` extended with the `apps/` and `crates/cfe*` boundaries. `AGENTS.md` also gains the rule that Aldis is used under `apps/` only.

**Done when:** this document's status is "accepted" and the Aldis issues exist.

### WP-1 cFE bindings and app skeleton (L)

| Task | Output |
|---|---|
| `node_defs/`: `targets.cmake`, configuration, startup script; `make node` builds it in the container | `node_defs/`, `Makefile`, `scripts/` |
| `cfe-sys` generated for the functions of F18 and C5, plus EVS and MSG; the check that it is current | `crates/cfe-sys` |
| `cfe`: `es`, `evs`, `sb`, `msg`, `osal::TcpStream`, each with host tests against a C stub where the wrapper has logic | `crates/cfe` |
| `link_lab` skeleton: loads, registers with EVS, creates its pipe, sends one event, runs the ES run loop | `apps/link_lab` |
| Symbol visibility: each app library exports only its entry point; a check in the style of `check-symbols.sh` | `scripts/` |

**Done when:** `make node` starts `core-cpu1` with `link_lab` loaded on both the stock and the Rust MSG module, the app's start-up event appears in the console log, and the symbol check passes with a second Rust app library loaded.

### WP-2 Harness node protocol and TCP transport (M, Aldis)

ACFS WP-2 as written: the protocol of §8.2, both transports, the launcher, the run manifest, and a scripted out-of-process test node. One addition: the launcher takes the path of an external `core-cpu1` staging directory.

**Done when:** an out-of-process test node reproduces `a0-empty` over TCP, and cfs-rs can invoke the harness at the pinned revision.

### WP-3 step_lab (M)

The node protocol client over `osal::TcpStream`; the `START`, wake, and `DONE` messages; the sequencing of §4.5 with apps that produce nothing; the pipe-depth check of §9.

**Done when:** `make node` reproduces `a0-empty` twice with the cFS node as the flight node.

### WP-4 link-lab-core (L, host only)

The core of §4.3 at all three levels, with the unit and differential tests of §4.6. No cFS is needed, so this runs in parallel with WP-1 to WP-3 and is where most Aldis API feedback will come from.

**Done when:** the differential tests reproduce the flight-side link records of every `a1-*`, `a2-*`, and `a3-*` trace on the host, and `make check` passes with the flight lint set.

### WP-5 Packets (M)

`scn_lab` on `refapp-flight`'s modules; `link_lab` glue at level `Packets`; the messages of §4.4.

**Done when:** `make node` reproduces every `a1-*` trace twice, byte for byte, on both MSG modules.

### WP-6 Frames (M)

`link_lab` glue at level `Frames`; stack size measured and set.

**Done when:** `make node` reproduces every `a2-*` trace twice.

### WP-7 COP-1 (S)

Level `Cop1`. Most of the work is already in WP-4.

**Done when:** `make node` reproduces every `a3-*` trace twice, the fault cases included.

### WP-8 Review and exit (S)

Panic-freedom and heap checks on the app libraries; `apps/link_lab/README.md` for an integrator; the Aldis API feedback issues listed; the §8 options decided or deferred.

## 8. After the first version

| Option | Value |
|---|---|
| Native mode: cFE's own telemetry and commands over TM and TC frames, in real time, to a ground system (Aldis already has a Yamcs interop directory) | The feature a cFS user would actually want; a demonstration, since no trace covers it |
| Routing and managed parameters from a cFE table | Exercises TBL; makes the app configurable without a rebuild |
| AOS and USLP levels | The scenario's `a4-*` cases |
| `scn_lab` split into command, housekeeping, and payload apps | More Software Bus integration |
| Move `apps/link_lab` to its own repository | Once the `cfe` crate's API is stable |

## 9. Risks

| Risk | Mitigation |
|---|---|
| A-1 does not happen: ACFS is on hold in Aldis and the harness has no out-of-process protocol | It is the critical path; raise it in WP-0. WP-1 and WP-4 do not depend on it |
| Two unknowns at once: a new binding layer and the cFS lockstep problems | WP-3 proves the lockstep with empty apps before any link logic runs on cFS; WP-4 proves the link logic with no cFS |
| Record order differs from the Rust node within an instant | The three rules of §4.5; the comparator prints the first differing record; the Rust node is the reference |
| Several Rust static libraries in one process: duplicate global symbols, two panic handlers | Hidden visibility and an export check, proven in WP-1 (§4.7) |
| OSAL truncates a pipe to `fs.mqueue.msg_max` without an error when unprivileged (F16) | Set the value for the container; `step_lab` checks the pipe depths at start-up and fails the run; the per-packet hand-off keeps occupancy low |
| The app's `Link` state overflows the task stack | Measured in WP-6; stack size set in the startup script (C1) |
| `scn_lab` cannot reproduce the node's step order from public modules | A-3; otherwise re-implement the handler sequencing and rely on the traces to catch differences |
| `cfe-sys` drifts from the configured headers | Regenerated and compared by `make node` (L3) |
| `link-lab-core` ends up a copy of `refapp-flight`'s link modules | Accepted; the evidence is the independent reproduction and the feedback issues. L4 can be reversed |
| The pinned Aldis revision moves under the apps | One pin (L9); a bump re-runs every case |
| `bindgen` needs `libclang` in the image and is a new tool | Maintainer decision L11; the generated file is committed, so only `make node` needs it |

# 🏛️ Axiom Engine (액시엄) v1.4.0

[![CI / Guard Verification](https://github.com/Hyedelle/Axiom/actions/workflows/deploy-pages.yml/badge.svg)](https://github.com/Hyedelle/Axiom/actions)
[![Live Web Demo](https://img.shields.io/badge/Live%20Demo-GitHub%20Pages-brightgreen?style=flat&logo=googlechrome)](https://hyedelle.github.io/Axiom/)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/License-MIT%20OR%20Apache--2.0-blue.svg)](LICENSE)
[![Rust 2021](https://img.shields.io/badge/Rust-1.80%2B-orange.svg?logo=rust)](https://www.rust-lang.org)
[![WASM](https://img.shields.io/badge/WebAssembly-650KB-blueviolet?logo=webassembly)](https://hyedelle.github.io/Axiom/)

> **"수학적 공리(Axiom)처럼 오차 없는 절대 결정론과 극한의 L1 캐시 효율을 지향하는 AI-Native 범용 게임/시뮬레이션 엔진"**  
> **Creator:** Hyedelle  
> **Core Stack:** Pure Rust (`#![no_std]` Core) · Data-Oriented Design (DOD) · Q32.16 Fixed-Point · Native Desktop Studio (`minifb`) · WebRTC DataChannel · WebAssembly  
> **Live Web Player:** **[https://hyedelle.github.io/Axiom/](https://hyedelle.github.io/Axiom/)**

---

## 🖥️ 1인 인디 개발자 네이티브 스튜디오 (Desktop Studio)

외부 웹 브라우저 없이, 로컬 데스크톱에서 단 한 줄로 60/144 FPS 인터랙티브 게임 창을 즉시 띄우고 실시간 마우스/키보드로 게임을 개발 및 플레이할 수 있습니다.

```powershell
# Windows 네이티브 데스크톱 스튜디오 실행 (1초 팝업)
cargo run -p axiom-cli
```

* **16x16 픽셀 스프라이트 아틀라스**: 단색 네모 탈피, 4프레임 벨트 이동/채굴기 회전/제련소 불꽃/하트 펄스/고블린 픽셀 아트 애니메이션.
* **디스크 퀵세이브/퀵로드**: `F5` 원클릭 빠른 저장 / `F9` 빠른 불러오기 (`saves/quick.axiom` 100% 무손실 바이너리).
* **실시간 인게임 프로파일러**: `F3` 키로 마이크로초 단위 시뮬레이션/렌더링 시간, FPS, 엔티티 수, FNV-1a 상태 해시, 60프레임 스파크라인 바 차트 토글.
* **완전한 조작계**: 마우스 좌클릭(배치), 우클릭(철거), WASD(이동), 1~8키(도구 선택), R키(회전), ESC(종료).

---

## 🎮 브라우저 원클릭 라이브 데모 (Live Web Demo)

별도의 설치 없이 브라우저에서 즉시 엔진의 결정론적 시뮬레이션과 넷코드를 60 FPS로 체험할 수 있습니다.

👉 **[Axiom Live Interactive Demo 실행하기](https://hyedelle.github.io/Axiom/)**

### 🕹️ 데모 탑재 모드
| 모드 (Mode) | 핵심 기술 및 시뮬레이션 내용 |
|---|---|
| **🌐 Co-op Factory** | **WebRTC DataChannel 2인 P2P 팩토리 협동 건설**: 0ms 입력 지연 GGPO 롤백 넷코드, 24B UDP 바이너리 패킷 통신, 0~300ms 지연/패킷 드랍 시뮬레이션, 지연 도착 시 실시간 프레임 롤백 및 재시뮬레이션 |
| **🏭 Factory Defense** | **자동화 공장 물류 & 타워 디펜스**: 32x32 타일 그리드, 채굴기·컨베이어 벨트·투입기·제련소 자율 생산 루프 + 탄약 자동 보급 포탑 및 몬스터 웨이브 방어 |
| **🌍 Open World** | **절차적 청크 스트리밍 & 100,000 NPC 생태계**: 무한 청크 공간 인덱서 + 10만 NPC 활동 상태 실시간 히트맵 미니맵 (0.58ms 연산) |
| **⚔️ Battle Arena** | **100 vs 100 대규모 자동 전투**: 순수 Q32.16 고정소수점 물리 및 충돌 판정, 틱당 0.38ms 연산 |
| **💡 Open World RPG** | **A\* 경로 탐색 AI & ATB 전투**: 64x64 미로 실시간 쿼리당 37.5µs 길찾기, 퀘스트 저널 FSM, 원소 속성 상성 전투 |

---

## 🌌 3대 엔지니어링 철학 (The 3 Axioms)

### 1. Absolute Determinism (부동소수점 없는 절대 결정론)
* `axiom-core`는 `#![no_std]` 기반으로 부동소수점(`f32`, `f64`)을 원천 배제하고 정밀 Q32.16 고정소수점 연산만을 사용합니다.
* 무작위 난수는 시드 기반 정수 PRNG(SplitMix64)로 엄격히 격리되며, 시스템 시간(`SystemTime`) 직접 참조가 금지됩니다.
* 동일한 시드와 입력 시퀀스가 주어지면 어느 플랫폼, 어느 OS에서 실행하더라도 1비트의 오차도 없이 100% 동일한 상태 해시($NextState = f(CurrentState, Input)$)를 산출합니다.

### 2. Cache-Line Resident (L1 캐시라인 64B 극한 압축)
* 포인터 체이싱(Pointer Chasing)을 철저히 배제하고, CPU 캐시 라인(64바이트) 규격에 완벽히 정렬된 플랫 배열(Flat Array / SoA) 형태를 우선합니다.
* `TileCell`, `TypeDefinition`, `RecipeDefinition`, `CombatUnit`, `Npc` 등 모든 핵심 상태 구조체가 `#[repr(C, align(64))]` 규격을 엄수하여 힙 할당 0(Zero Allocation)과 L1 캐시 미스 최소화를 달성합니다.

### 3. AI-Agent Native Architecture (무인 에이전트 자율 시공)
* 인간용 무거운 GUI 에디터 대신 100% 텍스트 코드, 선언적 스키마 데이터, 그리고 자동화된 헌법 가드 스크립트(`tools/guard.ps1`)로 통제됩니다.
* 총괄 관제탑(Antigravity)과 현장 시공팀(Claude Code)이 `docs/tasks.md`와 `docs/audit-report.md`를 통해 협업하며 기능 개발부터 감사, 릴리즈까지 무인 자율 파이프라인으로 수행합니다.

---

## 🏗️ 시스템 아키텍처 (Universal Engine SDK)

```
┌────────────────────────────────────────────────────────────────────────┐
│                   Axiom Universal Engine SDK Layer                     │
├──────────────────────────────────┬─────────────────────────────────────┤
│      crates/axiom-core/schema    │     crates/axiom-core/tile_entity   │
│  - 64B TypeDefinition 슬랩      │  - 64B TileCell (인벤토리/상태/스탯) │
│  - 64B RecipeDefinition 레시피   │  - TileGrid<W, H> 고정 무할당 그리드│
│  - TypeRegistry / RecipeRegistry │  - O(1) 좌표 접근 & 이웃 순회       │
│  - FNV-1a 결정론적 스키마 해시   │  - process_recipes 데이터 변환 엔진 │
├──────────────────────────────────┴─────────────────────────────────────┤
│                    crates/axiom-sdk (Universal C-ABI)                  │
│   - axiom_engine_create / destroy / tick / state_hash                  │
│   - axiom_schema_register_type / recipe (런타임 데이터 주입)           │
│   - axiom_grid_set_tile / get_tile / push_item / pop_item              │
│   - axiom_grid_net_* (P2P 그리드 롤백 네트워킹 C-ABI 10종)             │
│   - 제로-카피(Zero-Copy) 64B 버퍼 포인터 메모리 블릿 매핑              │
├────────────────────────────────────────────────────────────────────────┤
│                 WebRTC DataChannel P2P Networking                      │
│   - 24B Fixed Unreliable/Unordered UDP Packet                          │
│   - GridRollbackSession: 64-bit frame_tags 탑재 무한 링 버퍼           │
│   - Zero Input Lag 로컬 반응 + 지연 패킷 수신 시 자동 GGPO 롤백 재시뮬 │
└────────────────────────────────────────────────────────────────────────┘
```

---

## ⚡ 벤치마크 및 실측 성능 (Benchmarks)

| 항목 | 실측치 | 비고 |
|---|---|---|
| **통합 헤드리스 틱 연산** | **0.567 ms (1,761 FPS)** | 10만 NPC + 무한 청크 + 4,096 물류셀 + 200기 전투 동시 연산 |
| **100,000 NPC 스케줄링** | **0.587 ms** | 60Hz 예산(16.6ms) 대비 28배 고속 연산 |
| **4,096 공장 셀 물류 시뮬레이션** | **0.071 ms (71 µs)** | 채굴·벨트·투입·제련 전 과정 힙 할당 제로 |
| **공간 그리드 이웃 쿼리** | **0.395 µs (395 ns)** | 10,000개 엔티티 대상 초고속 공간 탐색 |
| **2D A\* 길찾기 AI** | **37.5 µs / 쿼리** | 초당 26,000회 연산, O(1) 에포크 태그 리셋 |
| **무손실 세이브/로드** | **세이브 1.55ms / 로드 2.16ms** | 10만 NPC + 청크 6.13MB 바이너리 100% 비트 일치 복원 |
| **WASM 프레임버퍼 블릿** | **0.05 ms 미만** | 더블 버퍼링 기반 브라우저 60 FPS 유지 |

---

## 📁 크레이트 구성 (Workspace Structure)

```text
Axiom/
├── crates/
│   ├── axiom-core/       # [Core] Q32.16 고정소수점 결정론, 롤백 넷코드, 64B 스키마 및 타일 그리드 (#![no_std])
│   ├── axiom-sdk/        # [SDK] 범용 C-ABI FFI 게이트웨이 및 언어 독립 바인딩 (Zero-Copy)
│   ├── axiom-game/       # [Game] 팩토리 물류, 거점 방어, 주민 노동 배정 등 레퍼런스 구현
│   ├── axiom-wasm/       # [WASM] 브라우저 WebRTC P2P DataChannel 및 HTML5 Canvas 고속 렌더러
│   ├── axiom-render/     # [Graphics] 초경량 소프트웨어 프레임버퍼 래스터라이저
│   └── axiom-cli/        # [CLI] 헤드리스 시뮬레이션, 리플레이어, 벤치마크 도구
├── web/                  # GitHub Pages 배포용 웹 플레이어 산출물 (WASM + HTML5)
├── tools/                # 헌법 검증기(guard.ps1), 모니터링 데몬(monitor.ps1)
└── docs/                 # 아키텍처, 감사 보고서, 작업 지시서, 기술 결정 기록
```

---

## 🚀 빠른 시작 (Quick Start)

### 요구 사양
- Rust 1.80+ (2021 edition)
- WebAssembly 타깃: `rustup target add wasm32-unknown-unknown`

### 1. 전체 무결성 검증 (헌법 가드)
```powershell
powershell -ExecutionPolicy Bypass -File tools/guard.ps1
```

### 2. 로컬 웹 플레이어 빌드 및 실행
```powershell
# WASM 빌드
cargo build --package axiom-wasm --target wasm32-unknown-unknown --release
Copy-Item target/wasm32-unknown-unknown/release/axiom_wasm.wasm web/axiom_wasm.wasm -Force

# 로컬 웹 서버 실행 (Python 3 기준)
python -m http.server 8080 --directory web
# 브라우저에서 http://localhost:8080 접속
```

### 3. 헤드리스 통합 시뮬레이션 실행
```powershell
cargo run --package axiom-cli --release
```

---

## 📜 라이선스
Axiom Engine은 [MIT License](LICENSE) 또는 [Apache-2.0 License](LICENSE) 듀얼 라이선스로 배포됩니다.

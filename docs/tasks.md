# 📋 Axiom Task Queue (작업 지시서)

> **규칙:** 이 파일의 `▶ [CURRENT]` 줄이 현재 진행 중인 단 하나의 작업입니다.  
> Claude Code는 이 한 줄만 읽고 작업을 수행하며, 작업이 끝나고 `tools/guard.ps1` 통과 시 커밋 후 즉시 `/exit`로 세션을 종료합니다.

---

## ▶ [CURRENT]
- [ ] **T-048: 범용 C-ABI 및 WASM Engine SDK 아키텍처 구축 (`crates/axiom-sdk`)**
  - **목표:**
    - 외부 언어(TypeScript, C#, Python 등) 및 WASM 캔버스에서 엔진을 직접 제어할 수 있는 **데이터 드리븐 Engine SDK 크레이트(`axiom-sdk`)** 생성.
    - 엔진 생명주기 API: `axiom_engine_create`, `axiom_engine_destroy`, `axiom_engine_tick`, `axiom_engine_state_hash`.
    - 스키마 등록 C-ABI: `axiom_schema_register_type`, `axiom_schema_register_recipe`.
    - 타일 엔티티 그리드 조작 C-ABI: `axiom_grid_set_tile`, `axiom_grid_get_tile`, `axiom_grid_push_item`, `axiom_grid_pop_item`.
    - 64바이트 정렬 메모리 포인터 직접 전달로 제로-카피(Zero-Copy) 버퍼 인터페이스 구현.
    - 100% 무부동소수점, 무할당, 결정론적 C-ABI 호출 검증.
  - **시공:** Antigravity (총괄 관제탑)
  - **검증:** `cargo test -p axiom-sdk` 및 `tools/guard.ps1` 통과

---

## ⏳ [BACKLOG]
- [ ] **T-049: 팩토리 RPG를 신규 데이터 드리븐 SDK 위의 예제 게임으로 리팩토링**

---

## 📜 [COMPLETED]
- [x] **T-047: 범용 타일 엔티티 및 공간 그리드 컴포넌트 파이프라인 (`crates/axiom-core/src/tile_entity.rs`)** (`crates/axiom-core/src/tile_entity.rs`, `TileCell` 64B 정렬 레코드, `TileGrid<W, H>` 고정 슬랩 2D 무할당 평면 그리드, O(1) 좌표 접근 및 이웃 탐색, `process_recipes` 데이터 드리븐 제작 파이프라인, FNV-1a 그리드 결정론 해시 검증, 5개 신규 단위 테스트 및 무부동소수점 무결성 검증 완료)
- [x] **T-046: 데이터 드리븐 스키마 정의 및 무할당 룩업 테이블 (`crates/axiom-core/src/schema.rs`)** (`crates/axiom-core/src/schema.rs`, `TypeDefinition` 64B 정렬 레코드, `RecipeDefinition` 64B 변환 규칙, `SchemaHeader` 64B 바이너리 헤더, `TypeRegistry<CAP>`/`RecipeRegistry<CAP>` 고정 슬랩 무할당 O(1) 룩업, FNV-1a 무할당 해시, 직렬화/역직렬화 및 31개 단위 테스트 100% 통과, 무부동소수점 무결성 검증 완료)
- [x] **T-045: 오픈월드 팩토리 RPG 통합 마일스톤 완결 및 프로덕션 v1.1.0 릴리즈** (`crates/axiom-game/src/defense.rs` 성벽 회피 미로 길찾기(Mazing) 및 성벽 집중 타격 돌파 로직 추가, `crates/axiom-wasm/index.html` 하드웨어 오프스크린 더블 버퍼링 및 고속 픽셀 블릿(0.05ms) 적용, `Cargo.toml` v1.1.0 공식 판올림, `docs/architecture.md` Section 13 등재, `docs/decisions.md` [D-014] 채택, `docs/audit-report.md` 공식 종합 감사 승인, GitHub Pages 원클릭 라이브 배포 완료)
- [x] **T-044: WebAssembly 웹 캔버스 팩토리 기지 대화형 건설 인터페이스 릴리즈** (`crates/axiom-wasm/src/lib.rs`, `axiom_factory_*` C-ABI 15개 엔드포인트 연동, 32x32 기지 캔버스 실시간 래스터라이징(벨트·아이템·채굴기·제련소 연기·포탑 궤적선·몬스터 체력바·베이스 하트 코어), `index.html` 팩토리 건설 툴바(드릴/벨트/투입기/제련소/상자/벽/포탑/철거/회전), 실시간 마우스 클릭 및 터치 배치, 16명 주민 자동 배치 연동, 11개 단위 테스트 및 100틱 비트 일치 결정론 통과, 581KB 경량 WASM 빌드 및 GitHub Pages 원클릭 라이브 배포 완료)
- [x] **T-043: 거점 방어(Base Defense) & 몬스터 웨이브 오토배틀러 연동** (`crates/axiom-game/src/defense.rs`, `StructureKind::Wall`/`Turret`/`BaseHeart` 16B 페이로드로 `FactoryStructure` 64B 셀 유지, `FactoryGrid::place_wall/place_turret/place_base_heart` 및 `WorldFactory::*_at` 배치, 포탑 탄약(철광석 100%/철판 150%/구리판 200%) 벨트 직결·인서터 투입·인접 체스트 자동 보급(`TURRET_AMMO_CAP` 20), `WaveDirector<CAP>` 64B 정렬 `repr(C)` 시드 `Rng` 가장자리 스폰 주기 웨이브(10틱 첫 웨이브, 50틱 간격, 4+2n 성장), `axiom_core::combat::BattleArena`/`CombatUnit` 몬스터 슬랩 연동, 정수 제곱거리 포탑 자동 사격 및 몬스터 최근접 방어 타일 공격, 벽/포탑 `hp` 소진 시 잔해 `Chest`(석재 4/철판 6) 생성, 하트 파괴 시 `base_fallen`, 힙 제로 `tick`/`tick_world`, 8개 단위 테스트 및 100틱 2웨이브 10기 전멸·하트 무피해 100% 비트 일치 해시 `0xF4D127AA8361ED57` 결정론 검증 통과)
- [x] **T-042: 10만 NPC 주민 영입 및 공장 노동/물류 자동화 배정 시스템** (`crates/axiom-game/src/worker.rs`, `WorkerRole` Miner/SmelterOperator/Hauler/Technician, `WorkerAssignment` 64B 정렬 `repr(C)` 레코드, `ColonyWorkforce<CAP>` 플랫 슬랩 고용/배정/해제, `axiom_core::Npc` 에너지 ↔ 스태미나 양방향 동기화(`hire`/`sync_npc`), 정수 생산성 버프(기본 +25%/+20% + 숙련도 5%/레벨) `MiningDrill`/`Smelter`/`Inserter`/`Belt` 속도 적용, 스태미나 소모(2/틱)·휴식(5/틱) 히스테리시스(100/600), `Hauler` 인력 물류 운반 자원 보존 검증, `WorldFactory` 청크 퇴출 시 Idle 폴백, 힙 제로 `tick`/`tick_world`, 13개 단위 테스트 및 100틱 100% 비트 일치 해시 `0xE3FD24FCCCC2FD94` 결정론 검증 통과)
- [x] **T-041: 오픈월드 무한 청크 매장지 탐험과 팩토리 기지 확장 연동** (`crates/axiom-game/src/world_factory.rs`, `WorldFactory<CHUNK_SIZE, ACTIVE_CHUNKS>`, 절차적 시드 기반 `generate_ore_veins` 4대 광맥 스폰, 청크 경계 `CrossChunkLogistics` 무손실 벨트 아이템 이송, 플레이어 원거리 이동 청크 퇴출 시에도 기지 생산 불변 보존 검증, 100틱 다중 청크 100% 비트 일치 해시 `0x199049755F60B310` 결정론 검증 통과)
- [x] **T-040: crates/axiom-game 자원 채굴 및 자동화 물류 기지 건설 루프 프로토타입** (`axiom-game` 신규 크레이트, `FactoryStructure` 64B 정렬 셀, `FactoryGrid<W, H>`, `MiningDrill`/`Belt`/`Inserter`/`Smelter`/`Chest`, 힙 제로 4단계 `step()`, [채굴 ➔ 벨트 ➔ 투입 ➔ 제련 ➔ 철판] 100틱 루프 철판 **47장** 적재 및 자원 보존 검증, 32x32 그리드 틱당 **6.1 µs**, 100틱 양 그리드 100% 비트 일치 해시 `0xF62F1795D5860647` 결정론 검증 통과)
- [x] **T-039: 웹(WASM) 즉시 배포 & 라이브 피드백 수집 시스템 구축** (`crates/axiom-wasm/index.html`, `web/`, `.github/workflows/deploy-pages.yml`, `tools/deploy-pages.ps1`, 모바일 D-Pad 터치 조작, 스펙 뱃지, 실시간 GitHub Issue 피드백 모달, 원클릭 GitHub Pages 배포 파이프라인 완성)
- [x] **T-038: Axiom 프로덕션 릴리즈 및 최종 디렉터 인도 보고서** (`Cargo.toml` v1.0.0 판올림, `docs/architecture.md` Section 12 종합 벤치마크 등재, `docs/decisions.md` [D-012] 채택, `docs/audit-report.md` 공식 감사 승인, `docs/dashboard.md` 최신화 완료 및 디렉터 인도 보고서 발행 완료)
- [x] **T-037: 완전 자율 운영 및 모니터링 데몬 시스템 완성** (`tools/monitor.ps1`, `crates/axiom-cli/src/telemetry.rs`, `docs/dashboard.md`, 실시간 엔진 상태/커밋/테스트/성능 모니터링 데몬 및 자동 갱신 검증 완료)
- [x] **T-036: Milestone 6 대규모 오픈월드 멀티플레이어 완결 감사 및 v0.6.0 릴리즈** (`Cargo.toml` v0.6.0 판올림, `docs/architecture.md` Section 11 등재, `docs/decisions.md` [D-011] 채택, `docs/audit-report.md` 공식 종합 감사 승인 완료)
- [x] **T-035: 오픈월드 RPG 멀티플레이어 협동 탐험 및 퀘스트 공유 시스템** (`QuestJournal`, 파티원 분담 토벌/채집 퀘스트 델타(`QuestDelta`) P2P 네트워크 동기화, 지연 패킷 도착 및 퀘스트 완료 후 양방향 피어 간 100% 비트 일치 저널 해시 합의 검증 통과)
- [x] **T-034: 오픈월드 RPG 멀티플레이어 청크 동적 호스팅 & 동기화** (`ChunkManager`, 분산 청크 스트리밍, P2P 지형 수정 델타(`TerrainDelta`) 전송 및 타임스탬프 기반 충돌 해결 중재 후 양방향 피어 간 100% 비트 일치 지형 해시 검증 통과)
- [x] **T-033: 오픈월드 RPG 멀티플레이어 3D 공간 음향 및 보이스 패킷 시뮬레이션** (`SpatialAudioSimulator`, 청자 좌표별 상대적 3D 거리 감쇄, 좌우 스테레오 패닝 대칭성 검증, 근접 보이스 패킷 가청 범위 판정 및 100프레임 무오차 결정론 검증 통과)
- [x] **T-032: Milestone 5 멀티플레이어 롤백 네트워킹 완결 감사 및 v0.5.0 릴리즈** (`Cargo.toml` v0.5.0 판올림, `docs/architecture.md` Section 10 등재, `docs/decisions.md` [D-010] 채택, `docs/audit-report.md` 공식 감사 승인 완료)
- [x] **T-031: 오픈월드 RPG 멀티플레이어 P2P 협동 전투 데모** (`MultiplayerRaidPeer`, 2인 협동 P2P 레이드 보스 드래곤 토벌, 지연된 원격 명령 수신 시 과거 프레임으로 롤백 및 재시뮬레이션 후 양방향 피어 간 100% 비트 일치 해시 합의 검증 통과)
- [x] **T-030: WebAssembly 브라우저 멀티플레이어 데이터채널 연동 인터페이스** (`axiom-wasm`, `axiom_net_*` C-ABI 바인딩, WebRTC DataChannel 원격 입력 수신 시 지연 롤백 트리거 및 재시뮬레이션, 브라우저 2인 P2P 롤백 넷코드 검증 통과)
- [x] **T-029: 네트워크 멀티플레이어 롤백 동기화 실전 통합 테스트** (`RollbackSession::<128, 32>`, `SimulatedNetworkPipe`, 200ms 대륙간 지연 환경 100프레임 동기화 및 15% 패킷 드랍/재전송 복구 환경에서 피어 간 100% 비트 일치 해시 합의 검증 통과)
- [x] **T-028: Milestone 4 오픈월드 완결 감사 보고서 및 엔진 v0.4.0 릴리즈** (`Cargo.toml` v0.4.0 판올림, `docs/architecture.md` Section 9 등재, `docs/decisions.md` [D-009] 채택, `docs/audit-report.md` 공식 종합 감사 승인 완료)
- [x] **T-027: 웹 캔버스(WASM) 기반 대화형 RPG 비주얼 플레이어 릴리즈** (`axiom-wasm`, `axiom_rpg_*` C-ABI 바인딩, **176 KB** WebAssembly 바이너리 빌드, HTML5 Canvas 60fps 대화형 뷰어: 실시간 10만 NPC 생태계 + 캔버스 클릭 A* 길찾기 AI + 퀘스트 대화 및 ATB 전투 HUD 오버레이 연동 검증 통과)
- [x] **T-026: 오픈월드 RPG 완결 데모 (무한 탐험 + 길찾기 AI + 퀘스트 + ATB 전투)** (`OpenWorldRpgSession`, `RpgReport`, 10만 NPC + 무한 청크 + 퀘스트 대화 트리 + 2D A* 길찾기 AI + ATB 원소 전투 + 3D 공간 음향 + 6.13MB 무손실 바이너리 저장/로드 100% 비트 일치 복원 및 크로스 세션 결정론 검증 통과)
- [x] **T-025: Milestone 3 종합 회고 및 엔진 v0.3.0 배포 릴리즈** (`Cargo.toml` v0.3.0 판올림, `docs/architecture.md`, `docs/decisions.md` [D-008], `docs/audit-report.md` 공식 감사 보고서 발행 완료)
- [x] **T-024: 오픈월드 오디오/사운드 이벤트 헤드리스 시뮬레이터** (`SpatialAudioSimulator`, 역제곱 거리 감쇄 및 스테레오 패닝, 1,000개 사운드 이벤트 공간화 믹싱 **47.3 µs = 0.047ms** 만에 완료 — 이벤트당 **0.05 µs** 초고속 연산 및 100% 비트 일치 검증 통과)
- [x] **T-023: 액티브 타임 전투(ATB) & 스킬 콤보 결정론적 이펙터 시스템** (`AtbUnit`, `AtbArena`, `CombatEffector`, 64B 정렬, 속성 상성 및 상태이상(Burn/Slow/Stun), 1,000틱 전투 틱당 **0.16 µs** 초고속 연산 및 100% 비트 일치 검증 통과)
- [x] **T-022: 대규모 오픈월드 저장/불러오기(Save/Load) 무손실 직렬화 벤치마크** (`WorldSerializer`, `SaveHeader`, 10만 NPC + 청크 + 퀘스트 6.13MB 바이너리 세이브 **1.55ms**, 로드 **2.16ms** 초고속 복원 및 100% 비트 일치 해시 복원 검증 통과)
- [x] **T-021: 고정소수점 2D A* 길찾기(NavMesh) 및 장애물 회피 시스템** (`PathFinder`, `FixedMinHeap`, O(1) 에포크 태그 리셋, 64x64 미로 지형 1,000회 길찾기 쿼리 37.5ms 만에 완료 — 쿼리당 **37.5 µs**, 초당 26,000회 연산 검증 통과)
- [x] **T-020: Milestone 3 분산 네트워크 롤백 넷코드(Lockstep/GGPO) 프로토타입** (`PlayerInputQueue`, `RollbackSession`, `SimulatedNetworkPipe`, 200ms 대기시간 및 예측 실패 환경 100프레임 시뮬레이션 후 양방향 피어 간 100% 비트 일치 마스터 해시 `0xF79CEBC644701D30` 동기화 검증 통과)
- [x] **T-019: WASM 웹 뷰어와 오픈월드 통합 데모 연동** (`axiom-wasm`, `FrameBuffer::render_npc_minimap`, HTML5 Canvas 실시간 10만 NPC 활동별 컬러 히트맵 및 무한 청크 실시간 탐험 WASM 바인딩 검증 통과)
- [x] **T-018: Phase 4 심층 플레이어 상호작용 및 퀘스트/다이얼로그 상태기계 설계** (`QuestRecord`, `QuestJournal`, `DialogueSession`, 64B 정렬, 퀘스트 수락·진행·완료 및 20틱 타임머신 롤백 100% 비트 일치 복원 검증 통과)
- [x] **T-017: Milestone 2 완성 보고서 및 아키텍처 문서 동기화** (`docs/architecture.md`, `docs/decisions.md` [D-007] 동기화 완료: 10만 NPC 생태계, 무한 청크 스트리밍, 4,096셀 공장 물류, 200기 전투 통합 **1,761 FPS** 실측 명세 공식 등재)
- [x] **T-016: 오픈월드 RPG 통합 헤드리스 플레이세션 검증** (`HeadlessSession`, 10만 NPC + 무한 청크 + 4,096셀 공장 물류 + 200유닛 전투를 틱당 **0.567ms = 567µs**에 동시 시뮬레이션 — 60fps 예산의 3.4%만 사용, **1,761 FPS** 속도 및 100% 비트 일치 결정론 마스터 해시 검증 통과)
- [x] **T-015: 타임머신 롤백과 청크 스트리밍 통합 검증** (`WorldSnapshot`, `WorldTimeMachine`, 청크 스트리밍 + 지형 변형 + NPC 스케줄 60틱 시뮬레이션 후 30틱 타임머신 롤백 100% 비트 일치 복원 및 분기 타임라인 검증 통과)
- [x] **T-014: Phase 4 10만 NPC 생태계 배경 시뮬레이션 프로토타입** (`Npc`, `NpcTable<1024>`, `NpcCrowdPool<100>`, 64B 캐시라인 정렬, 100,000 NPC 60틱 시뮬레이션 틱당 **0.587ms = 587µs** 달성 — 60Hz 16.6ms 대비 28배 빠름, 검증 통과)
- [x] **T-013: Phase 3 무한 청크(Chunk) 스트리밍 및 공간 인덱서** (`Chunk<32>`, `ChunkManager`, 고정 메모리 슬랩 청크 스트리밍, 1,000회 연속 이동 4,525개 청크 스트리밍 틱당 **0.14ms = 145µs**, 청크 생성당 **32µs** 초고속 실시간 스트리밍 검증 통과)
- [x] **T-012: Phase 2 자원 채굴 및 물류 벨트 시뮬레이션** (`LogisticsGrid<64, 64>`, `LogisticsCell` 32B 패킹, 4,096개 공장 셀 틱당 **0.071ms = 71µs** 초고속 물류 시뮬레이션 검증 통과)
- [x] **T-011: Phase 1 오토배틀러 완전 자율 AI 밸런싱 시뮬레이터** (`MonteCarloBalancer`, 10,000 대전 **187ms** 만에 완료, 기물 승률/통계 분석 보고서 `tmp/balance_report.md` 자율 발행)
- [x] **T-010: 타임머신 롤백 리플레이 뷰어 CLI 통합** (`AsciiView`, `TimeTravelDemo`, 30틱 진행 후 0.001ms 틱 10 롤백 및 아스키 뷰포트 시각화 검증 완료)
- [x] **T-009: 웹 브라우저(WASM) 및 실시간 뷰어 연동 파이프라인** (`axiom-wasm`, **44 KB** 초경량 WebAssembly 바이너리 빌드 및 HTML5 캔버스 60fps 무오차 뷰어 데모 완성)
- [x] **T-008: 시드 기반 절차적 맵 생성기(PCG) 프로토타입** (SplitMix64 정수 PRNG, `TileMap<W, H>`, 2단계 셀룰러 오토마타 지형 클러스터링, 100% 동일 시드 비트 일치 검증 통과)
- [x] **T-007: 오토배틀러 프로토타입 룰셋 구현** (`BattleArena`, 64B `CombatUnit`, 100 vs 100 대규모 전투 시뮬레이션 틱당 **0.38ms**, 100% 결정론적 전투 결과 검증 통과)
- [x] **T-006: 2D 그리드 공간 분할(Spatial Hash Grid) 슬랩 구현** (`SpatialGrid<64, 64, 16>`, 10,000개 엔티티 대상 이웃 쿼리 **0.395µs = 395나노초** 기록 달성)
- [x] **T-005: 헤드리스 CLI 리플레이어 (axiom-cli)** (1,000틱 입력 스트림 무오차 재현 및 해시 일관성 검증 통과)
- [x] **T-004: 초경량 렌더러 연동 (axiom-render)** (`FrameBuffer`, 1920x1080 뷰포트에 10,000개 고정소수점 엔티티 실시간 래스터라이징 검증)
- [x] **T-003: 10,000 엔티티 동시 틱 시뮬레이션 벤치마크** (10,000개 엔티티 60틱 시뮬레이션 **0.49ms** 완료, 틱당 **0.008ms** 기록 달성 — 60Hz 예산 대비 2,000배 빠름)
- [x] **T-002: L1 캐시 엔티티 배열(Entity Storage) 구현** (`EntityTable<1024>`, `EntitySlabPool`, 64B 정렬, 힙 할당 0)
- [x] **T-001: Milestone 1 결정론적 틱 코어 & 100틱 롤백 타임머신 구현** (`b34bfb6`)

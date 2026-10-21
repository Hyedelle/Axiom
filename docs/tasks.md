# 📋 Axiom Task Queue (작업 지시서)

> **규칙:** 이 파일의 `▶ [CURRENT]` 줄이 현재 진행 중인 단 하나의 작업입니다.  
> Claude Code는 이 한 줄만 읽고 작업을 수행하며, 작업이 끝나고 `tools/guard.ps1` 통과 시 커밋 후 즉시 `/exit`로 세션을 종료합니다.

---

## ▶ [CURRENT]
- [ ] **T-009: 웹 브라우저(WASM) 및 실시간 뷰어 연동 파이프라인**
  - **목표:** `crates/axiom-core` 및 `crates/axiom-render`를 `wasm32-unknown-unknown` 타겟으로 컴파일 가능하도록 검증하고, 브라우저 캔버스/HTML 또는 실시간 네이티브 창에서 60fps로 시뮬레이션 렌더링을 시각화할 수 있는 데모 구축
  - **검증:** `cargo check --target wasm32-unknown-unknown` 및 `tools/guard.ps1` 통과
  - **가드:** Float 배제, no_std 호환성 유지

---

## ⏳ [BACKLOG]
- [ ] **T-010: 타임머신 롤백 리플레이 뷰어 CLI 통합**
- [ ] **T-011: Phase 1 오토배틀러 완전 자율 AI 밸런싱 시뮬레이터 (Monte Carlo 10,000 Matches)**

---

## 📜 [COMPLETED]
- [x] **T-008: 시드 기반 절차적 맵 생성기(PCG) 프로토타입** (SplitMix64 정수 PRNG, `TileMap<W, H>`, 2단계 셀룰러 오토마타 지형 클러스터링, 100% 동일 시드 비트 일치 검증 통과)
- [x] **T-007: 오토배틀러 프로토타입 룰셋 구현** (`BattleArena`, 64B `CombatUnit`, 100 vs 100 대규모 전투 시뮬레이션 틱당 **0.38ms**, 100% 결정론적 전투 결과 검증 통과)
- [x] **T-006: 2D 그리드 공간 분할(Spatial Hash Grid) 슬랩 구현** (`SpatialGrid<64, 64, 16>`, 10,000개 엔티티 대상 이웃 쿼리 **0.395µs = 395나노초** 기록 달성)
- [x] **T-005: 헤드리스 CLI 리플레이어 (axiom-cli)** (1,000틱 입력 스트림 무오차 재현 및 해시 일관성 검증 통과)
- [x] **T-004: 초경량 렌더러 연동 (axiom-render)** (`FrameBuffer`, 1920x1080 뷰포트에 10,000개 고정소수점 엔티티 실시간 래스터라이징 검증)
- [x] **T-003: 10,000 엔티티 동시 틱 시뮬레이션 벤치마크** (10,000개 엔티티 60틱 시뮬레이션 **0.49ms** 완료, 틱당 **0.008ms** 기록 달성 — 60Hz 예산 대비 2,000배 빠름)
- [x] **T-002: L1 캐시 엔티티 배열(Entity Storage) 구현** (`EntityTable<1024>`, `EntitySlabPool`, 64B 정렬, 힙 할당 0)
- [x] **T-001: Milestone 1 결정론적 틱 코어 & 100틱 롤백 타임머신 구현** (`b34bfb6`)

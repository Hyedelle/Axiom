# 📋 Axiom Task Queue (작업 지시서)

> **규칙:** 이 파일의 `▶ [CURRENT]` 줄이 현재 진행 중인 단 하나의 작업입니다.  
> Claude Code는 이 한 줄만 읽고 작업을 수행하며, 작업이 끝나고 `tools/guard.ps1` 통과 시 커밋 후 즉시 `/exit`로 세션을 종료합니다.

---

## ▶ [CURRENT]
- [ ] **T-004: 초경량 렌더러 연동 (axiom-render with wgpu/Raylib)**
  - **목표:** `crates/axiom-render` 크레이트를 워크스페이스에 생성하고, 헤드리스 코어(`axiom-core`)의 10,000개 엔티티 위치 데이터를 받아 60fps로 화면에 점/사각형을 그리는 초경량 뷰어 프로토타입 구현
  - **검증:** `cargo test --workspace` 및 `tools/guard.ps1` 통과
  - **가드:** Float 및 외생 시계는 `axiom-render`에만 국한, `axiom-core`는 100% 순수 결정론 유지

---

## ⏳ [BACKLOG]
- [ ] **T-005: 헤드리스 CLI 리플레이어 (입력 로그 기반 타임머신 재현 도구)**
- [ ] **T-006: 2D 그리드 공간 분할(Spatial Hash Grid) 슬랩 구현 (충돌/이웃 쿼리)**

---

## 📜 [COMPLETED]
- [x] **T-003: 10,000 엔티티 동시 틱 시뮬레이션 벤치마크** (10,000개 엔티티 60틱 시뮬레이션 **0.49ms** 완료, 틱당 **0.008ms** 기록 달성 — 60Hz 예산 대비 2,000배 빠름)
- [x] **T-002: L1 캐시 엔티티 배열(Entity Storage) 구현** (`EntityTable<1024>`, `EntitySlabPool`, 64B 정렬, 힙 할당 0)
- [x] **T-001: Milestone 1 결정론적 틱 코어 & 100틱 롤백 타임머신 구현** (`b34bfb6`)

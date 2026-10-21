# 📋 Axiom Task Queue (작업 지시서)

> **규칙:** 이 파일의 `▶ [CURRENT]` 줄이 현재 진행 중인 단 하나의 작업입니다.  
> Claude Code는 이 한 줄만 읽고 작업을 수행하며, 작업이 끝나고 `tools/guard.ps1` 통과 시 커밋 후 즉시 `/exit`로 세션을 종료합니다.

---

## ▶ [CURRENT]
- [ ] **T-003: 10,000 엔티티 동시 틱 시뮬레이션 벤치마크**
  - **목표:** `crates/axiom-core`에서 10,000개 엔티티를 동시 업데이트(Kinematics + Hashing)하는 벤치마크 작성 및 60Hz(16.6ms) 대비 처리 시간(예: < 1ms) 계측
  - **검증:** `cargo test --release` 및 `tools/guard.ps1` 통과
  - **가드:** Float 사용 금지, 64바이트 캐시라인 정렬 유지

---

## ⏳ [BACKLOG]
- [ ] **T-004: 초경량 렌더러 연동 (axiom-render with wgpu/Raylib)**
- [ ] **T-005: 헤드리스 CLI 리플레이어 (입력 로그 기반 타임머신 재현 도구)**

---

## 📜 [COMPLETED]
- [x] **T-002: L1 캐시 엔티티 배열(Entity Storage) 구현** (`EntityTable<1024>`, 64B 정렬, 힙 할당 0, 순차 순회 검증 통과)
- [x] **T-001: Milestone 1 결정론적 틱 코어 & 100틱 롤백 타임머신 구현** (`b34bfb6`)

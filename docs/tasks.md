# Axiom Task Board (docs/tasks.md)

> **원칙 (Disk as Hub):** 모든 에이전트(Antigravity, Claude Code)는 이 게시판을 통해 작업을 인계받고 결과를 기록합니다.  
> **운영 수칙:** 1 태스크 = 1 커밋 = 1 세션 리셋 (`/exit`). 완료 시 즉시 커밋하고 세션을 종료합니다.

---

## 🚀 Now (진행 중인 작업)

| ID | 담당 | 내용 | 상태 | 외부 러너 검증 |
|---|---|---|---|---|
| CORE-002 | Claude Code / Antigravity | 64바이트 캐시라인 정렬 DOD 슬랩 할당기 설계 | Ready | `tools/run_test.ps1` |

---

## 📋 Backlog (대기열)

| ID | 우선순위 | 작업명 | 설명 | 비고 |
|---|---|---|---|---|
| CORE-003 | P1 | 고정소수점(Fixed-point) 2D/3D 벡터 수학 라이브러리 | 결정론적 위치/속도/충돌 계산을 위한 Q32.32 또는 Q16.16 연산 체계 | 부동소수점 절대 금지 |
| FFI-001  | P2 | Headless C-ABI / IPC 인터페이스 프로토타입 | 외부 클라이언트(GUI/렌더러/Python) 연동용 무오차 C ABI 인터페이스 정의 | `tech/ffi` 참고 |

---

## ✅ Done (완료된 작업)

| ID | 완료일 | 커밋 | 내용 | 검증 결과 |
|---|---|---|---|---|
| AX-001 | 2026-10-22 | Pending | Axiom 프로젝트 AX(Agent Experience) 환경 구축 (Disk as Hub, 격리 러너, 4중 Git Hook) | `tools/run_test.ps1` & Hook PASS |
| CORE-001 | 2026-10-22 | `b34bfb6` | Milestone 1: 결정론적 틱 시뮬레이션 및 100틱 롤백(Time-Travel) 단위 테스트 | `cargo test` PASS |

---

## 🚨 Audit Needed (관제탑 감사 및 긴급 처방 통로)
*(수백 줄 이상의 빌드 실패나 아키텍처 결함 발생 시 Antigravity가 [docs/audit-report.md](docs/audit-report.md)에 처방전을 발행합니다.)*
- 현재 긴급 이슈 없음.

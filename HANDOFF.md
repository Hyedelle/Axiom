# 🤝 Axiom Engine — 에이전트 인수인계서 (HANDOFF.md)

> **일시:** 2026-10-22  
> **인계자:** Research Lab 수석 연구원 (Antigravity)  
> **인수자:** Axiom 전담 수석 엔지니어 (새로운 Antigravity / Claude Code 세션)  
> **디렉터:** Hyedelle (반도체 엔지니어, 1인 개발자)  
> **작업 디렉터리:** `C:\Dreamhub\Engine\Axiom`

---

## 1. 프로젝트 탄생 배경과 비전

기존의 Unity, Unreal 같은 상용 게임 엔진은 30년 전의 객체지향(OOP) 구조와 무거운 GUI에 갇혀 있어, 1인 개발자의 AI 에이전트(AX) 협업에 엄청난 병목(긴 컴파일, 토큰 폭발, 비결정론)을 유발했습니다.

**Axiom(액시엄)**은 반도체 하드웨어의 엄밀함과 Rust의 안전성을 결합하여 만든 **세상에 없던 차세대 특화 엔진**입니다.

### 3대 핵심 공리 (Axioms)
1. **Absolute Determinism (절대 결정론)**: 
   - $NextState = f(CurrentState, Input)$ 수학적 순수 함수 구조.
   - 1틱 단위 시간 되감기(Time-Travel)와 롤백 디버깅 0.001초 보장.
2. **Cache-Line Resident (L1/L2 극한 압축)**:
   - 포인터 체이싱 배제, 64바이트 캐시 라인 규격 맞춤형 데이터 지향 설계(DOD).
   - 단일 코어에서 10만 개 엔티티 렉 없이 동시 연산.
3. **AI-Agent Native (무인 에이전트 통제)**:
   - 사람용 GUI 에디터 0%. 순수 텍스트 코드와 데이터로 통제되는 완전 헤드리스(Headless) 엔진.

---

## 2. 디렉터(Hyedelle) 특성 및 협업 지침

* **엔지니어링 백그라운드**: 반도체 엔지니어로서 캐시, 파이프라인, 비트 연산, 불변식(Invariant)의 가치를 누구보다 깊게 이해하고 계십니다.
* **장비 & 라이선스**: 
  - Windows PC (Ryzen 3500X 6-core, 32GB RAM, GTX 1660 SUPER, D드라이브 1.7TB)
  - Claude Code Max ($200 CLI) + Google AI Pro (수백만 토큰 대형 분석)
* **토큰 방어 절대 원칙 (Clean Reset)**:
  - 거대한 로그를 대화창에 복사-붙여넣기하지 마십시오.
  - 1개의 태스크가 끝나고 테스트가 통과하면 **반드시 Git 커밋 후 세션을 리셋(`/exit`)**하는 1인 개발 AX 원칙을 준수해야 합니다.
  - 모든 에이전트 간 통신은 디스크 파일(`docs/tasks.md` 등)을 통해서만 수행합니다 (Disk as Hub).

---

## 3. 첫 번째 개발 마일스톤 (Milestone 1: The Core Beat)

Axiom 세션이 가장 먼저 착수해야 할 작업입니다:

* [x] **Task 1: Rust 워크스페이스 세팅 (`Cargo.toml`)**
  - `crates/axiom-core` (결정론적 틱 시뮬레이터 & 타임머신 버퍼)
* [x] **Task 2: 고정 소수점 / 무오차 틱 루프 구현**
  - 부동소수점 오차 없는 고정 틱(Fixed Tick, 60Hz) 상태 전이 구조체 정의
* [x] **Task 3: 100틱 롤백(Time-Travel) 단위 테스트 작성 (`cargo test`)**
  - "100틱 진행 ➔ 50틱 전으로 롤백 ➔ 다시 진행했을 때 정확히 동일한 해시값 산출" 검증 (0.01초 컷)

# 🏛️ Axiom Engine (액시엄)

> **"수학적 공리(Axiom)처럼 오차 없는 절대 결정론과 극한의 캐시 효율을 지향하는 AI-Native 커스텀 게임 엔진"**  
> **Creator:** Hyedelle  
> **Core Stack:** Rust · wgpu/Raylib · Data-Oriented Design (DOD) · Agent AX  
> **Created:** 2026-10-22  

---

## 🌌 3대 핵심 철학 (The 3 Axioms)

### 1. Absolute Determinism (절대 결정론 & 타임 트래블)
* 게임의 모든 상태 전이는 순수 함수($NextState = f(CurrentState, Input)$)로 정의됩니다.
* 1틱(Tick) 단위의 시간 되감기(Time-Travel)와 롤백 디버깅을 0.001초 만에 수행합니다.
* 1시간 플레이 로그도 단 몇 KB의 시드와 입력 시퀀스로 완벽 재현됩니다.

### 2. Cache-Line Resident (L1/L2 캐시 극한 압축)
* 포인터 체이싱(Pointer Chasing)을 철저히 배제하고, CPU 64바이트 캐시 라인 규격에 최적화된 데이터 지향 설계(DOD)를 따릅니다.
* 수만~수십만 개의 시뮬레이션 엔티티를 단일 코어에서도 렉 없이 초고속 연산합니다.

### 3. AI-Agent Native (무인 에이전트 특화)
* 사람을 위한 무거운 GUI 에디터를 걷어내고, 100% 텍스트 코드와 선언적 데이터로 제어됩니다.
* Claude Code Max와 Antigravity가 터미널에서 엔진과 게임플레이를 자율적으로 빌드하고 검증합니다.

---

## 📁 디렉터리 계획 (로드맵)

```text
Axiom/
├── crates/
│   ├── axiom-core/       # [Rust] 불변식 기반 결정론적 시뮬레이터 & 타임머신
│   ├── axiom-render/     # [Graphics] 초경량 wgpu / 렌더링 파이프라인
│   └── axiom-cli/        # [CLI] 에이전트 및 헤드리스 테스트용 명령줄 툴
├── docs/                 # 아키텍처 및 불변식(Invariants) 명세
└── examples/             # 1초 만에 뜨는 초소형 프로토타입 예제들
```

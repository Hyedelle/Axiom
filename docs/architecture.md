# Axiom Engine Architecture Specification (docs/architecture.md)

> **문서 버전:** 1.0.0  
> **최종 수정일:** 2026-10-22  
> **작성 주체:** Hyedelle (디렉터) & Antigravity (총괄 관제탑)  
> **위치:** `crates/axiom-core` 및 관련 서브시스템 명세

---

## 1. 핵심 철학 (The 3 Axioms)

Axiom 엔진은 기존 상용 엔진(Unity/Unreal)의 OOP 계층과 무거운 GUI 에디터가 유발하는 **비결정론, 긴 컴파일 시간, AI 에이전트 협업(AX) 병목**을 근본적으로 해결하기 위해 탄생했습니다.

1. **Absolute Determinism (절대 결정론)**
   - 상태 전이는 수학적 순수 함수로 정의됩니다:
     $$S_{t+1} = \text{Step}(S_t, I_t)$$
   - 동일한 입력 스트림이 주어지면 어느 머신, 어느 OS에서 실행해도 1비트의 오차 없이 동일한 상태 해시를 산출합니다.
2. **Cache-Line Resident (L1/L2 캐시라인 극한 상주)**
   - CPU 캐시라인(64바이트) 맞춤형 데이터 지향 설계(DOD).
   - 포인터 간접 참조(Pointer Chasing)를 원천 배제하여 단일 코어 10만 엔티티 연산을 L1/L2 캐시 내에서 무지연 처리합니다.
3. **AI-Agent Native (무인 에이전트 통제)**
   - 사람용 GUI 에디터 0%.
   - 모든 제어는 순수 코드, 직렬화된 데이터, CLI 인자, 격리된 외부 러너로 구동됩니다.

---

## 2. 메모리 기하구조 (Memory Geometry)

### 2.1 64바이트 캐시라인 정렬 (Hardware Alignment)
현대 x86-64 및 ARM64 아키텍처의 L1 데이터 캐시라인은 정확히 **64바이트**입니다.
Axiom의 모든 코어 엔티티는 이 하드웨어 규격을 1바이트의 오차도 없이 일치시킵니다.

```rust
#[repr(C, align(64))]
pub struct Entity {
    pub id: u32,                  // 4 bytes
    pub flags: u32,               // 4 bytes (Active, Alive, Visible 등)
    pub position: FixedVec2,      // 16 bytes (x: 8B, y: 8B)
    pub velocity: FixedVec2,      // 16 bytes (x: 8B, y: 8B)
    pub padding: [u8; 24],        // 24 bytes (캐시라인 64B 충진)
}
// sizeof(Entity) == 64
// alignof(Entity) == 64
```

### 2.2 슬랩 메모리 배치 (DOD Slab Allocation)
- 엔티티와 컴포넌트는 힙 상에 흩어지지 않고, 고정 크기 연속 슬랩(`[Entity; N]`) 배열로 배치됩니다.
- CPU 하드웨어 프리페처(Hardware Prefetcher)가 순차적 메모리 스트라이드를 완벽히 예측하여 캐시 미스(Cache Miss)를 0으로 수렴시킵니다.

---

## 3. 결정론 수학 체계 (Fixed-Point Math)

### 3.1 부동소수점 영구 금지 (No Float in Core)
- IEEE 754 부동소수점(`f32`, `f64`)은 컴파일러 최적화 플래그(FMA, fast-math)나 CPU 마이크로아키텍처에 따라 하위 비트 오차가 발생하므로 코어 사용을 엄격히 금지합니다.
- 모든 위치, 속도, 각도, 물리 연산은 **Q32.16 고정소수점 (`Fixed`)**으로 계산합니다:
  - 1.0 = $2^{16} = 65,536$
  - 정수 덧셈/뺄셈/곱셈/나눗셈 비트 시프트 연산으로 100% 무오차 보장.

---

## 4. 시간과 틱 파이프라인 (Time & Rollback)

### 4.1 고정 60Hz 틱 (Fixed Tick)
- 외생적인 벽시계(`SystemTime`, `Instant`)를 참조하지 않습니다.
- 시뮬레이션의 시간은 오직 단조 증가하는 정수 `Tick(u64)`으로만 흐릅니다.

### 4.2 타임머신 버퍼 (Time-Machine Ring Buffer)
- 과거 $N$틱의 `TickState`와 `InputState`를 순환 링 버퍼에 상시 기록합니다.
- 롤백 요청 시 0.001초 만에 과거 상태를 복원하고 현재 틱까지 리플레이(Re-simulate)를 수행합니다.

---

## 5. AX 협업 및 가드레일 (Agent Governance)

- **Disk as Hub**: [docs/tasks.md](tasks.md), [docs/audit-report.md](audit-report.md), [docs/decisions.md](decisions.md)
- **외부 격리 러너**: `.\tools\run_test.ps1` (로그를 `tmp/`로 격리, 결과 1줄 출력)
- **헌법 가드레일**: `admin/hooks/pre-commit` (Git 레벨 4대 게이트 자동 차단)
- **캐시 분리**: `tmp/`, `target/` ➔ `D:\axiom\cache\` 정션 링크

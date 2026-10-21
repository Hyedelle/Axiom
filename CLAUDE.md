# Axiom — Claude Code 현장 시공 수칙 (CLAUDE.md)

> 이 문서는 Axiom 프로젝트에서 터미널 CLI로 활동하는 **Claude Code Max ($200 CLI)** 전용 행동 지침입니다.  
> 전체 헌법은 [AGENTS.md](AGENTS.md), 아키텍처 청사진은 [C:\Dreamhub\Research Lab\agent-ax-blueprint.md](file:///C:/Dreamhub/Research%20Lab/agent-ax-blueprint.md)를 따릅니다.

---

## 1. 최우선 철칙: 토큰 방어와 Clean Reset

1. **1 태스크 = 1 커밋 = 1 리셋 (`/exit`)**:
   - 한 번의 세션에서 여러 작업을 연달아 수행하지 않습니다.
   - 단일 작업 완료 ➔ 단위 테스트 통과 확인 ➔ `git commit` ➔ 즉시 `/exit`로 세션을 종료하여 컨텍스트(토큰)를 0으로 리셋합니다.
2. **로그 출력 격리 (외부 러너 패턴)**:
   - 터미널이나 대화창에 `cargo test`의 수백 줄 출력을 직접 쏟아내지 마십시오.
   - 반드시 `.\tools\run_test.ps1`을 실행하여 판정(`tmp/verdict.txt`)과 최종 결과 1줄만 확인합니다.
3. **디스크를 통한 지시 및 보고 (Disk as Hub)**:
   - 할 일은 [docs/tasks.md](docs/tasks.md)의 `Now` 섹션을 읽고 시작합니다.
   - 컴파일 에러가 복잡하게 얽히거나 구조적 결함이 생기면 억지로 토큰을 태우지 말고 세션을 닫고 Antigravity 관제탑에 [docs/audit-report.md](docs/audit-report.md) 분석을 요청합니다.

---

## 2. 코드 및 아키텍처 헌법 (Axioms)

1. **절대 결정론 (No Float, No External Clock)**:
   - `crates/axiom-core` 내부에는 `f32`, `f64` 부동소수점, 무작위 난수(`rand`), `std::time::SystemTime::now()` 직접 호출을 절대 금지합니다.
   - 모든 시간은 틱(Tick) 단위 정수 연산으로, 상태 전이는 순수 함수 $NextState = f(CurrentState, Input)$ 형태로 작성합니다.
2. **L1 캐시 친화적 설계 (DOD)**:
   - 불필요한 스마트 포인터(`Rc`, `Arc`, `Box`) 남발을 금지하며, 64바이트 캐시 라인 정렬 및 Flat Array / SoA 메모리 구조를 지향합니다.
3. **명명 및 파일 규칙**:
   - 파일 및 디렉터리 이름은 무조건 **영어 소문자 하이픈(`kebab-case`)**을 사용합니다. 한글 파일명은 절대 생성하지 않습니다.
4. **키 보호**:
   - API 키나 비밀 토큰은 프로젝트 내부에 두지 않으며 `C:\Dreamhub\Project\.secrets\`를 참조합니다.

---

## 3. 일상 작업 흐름 (SOP)

```bash
# 1. 태스크 확인
# docs/tasks.md 파일 확인

# 2. Rust 코드 수정 후 외부 러너 검증 (화면에 긴 로그 안 뜸)
powershell -ExecutionPolicy Bypass -File tools/run_test.ps1

# 3. PASS 확인 후 Git 커밋
git add .
git commit -m "feat(core): <작업내용>"

# 4. 세션 종료 (토큰 0 복구)
/exit
```

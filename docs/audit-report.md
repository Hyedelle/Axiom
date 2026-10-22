# Axiom Architecture & Audit Report (docs/audit-report.md)

> **작성 주체:** 🏛️ Antigravity (Google Gemini - 총괄 관제탑 & 선행 연구소)  
> **수신 주체:** ⚡ Claude Code Max (현장 특수 시공팀) & Hyedelle (디렉터)  
> **목적:** 수만 줄 빌드 에러 로그, 아키텍처 결함, L1 캐시 위배 사항을 선행 분석하여 Claude Code가 10초 만에 수정할 수 있는 정밀 처방전 제공.

---

## 📌 최신 감사 요약 (Latest Audit)

- **일시:** 2026-10-22 12:20 KST
- **감사 대상:** Axiom Engine v1.3.0 WebRTC DataChannel 및 결정론적 그리드 롤백 넷코드 릴리즈 감사 (`axiom-core`, `axiom-sdk`, `axiom-wasm` + WebRTC P2P DataChannel)
- **판정:** 🟢 **ALL CLEAR — 100% 헌법 및 무부동소수점 무결성, 64-bit frame_tags 무한 링 버퍼, 16B GridAction, GGPO 그리드 롤백 통과 (v1.3.0 공식 승인)**
- **주요 검증 항목:**
  1. **부동소수점(`f32`, `f64`) 원천 배제**: `axiom-core`, `axiom-sdk`, `axiom-game`, `axiom-wasm` 전체 코드베이스 0건 (`tools/guard.ps1` 통과)
  2. **16B 캐시라인 정렬 `GridAction`**: 플레이어 ID, 액션 종류, X/Y, 타입 ID, 파라미터 1/2로 구성된 16바이트 정밀 레코드 및 컴파일 타임 크기 단언 검증 완료
  3. **64비트 `frame_tags` 무한 링 버퍼 탈동기화 방지**: `GridInputQueue` 및 `PlayerInputQueue` 링 버퍼 랩어라운드 발생 시 과거 프레임과 현재 프레임 혼동 완벽 차단
  4. **공간 그리드 분산 롤백 세션 (`GridRollbackSession`)**: 200ms 대륙간 네트워크 지연 및 15% 패킷 드랍 환경에서도 원격 액션 지연 도착 시 즉각 롤백 후 전방 재시뮬레이션을 수행하여 피어 간 100% 비트 일치 `state_hash` 합의 달성
  5. **범용 C-ABI SDK 바인딩 (`axiom_grid_net_*`)**: 10개 엔드포인트 연동 (세션 생성/해제, 액션 큐잉, 원격 패킷 수신, 롤백 스텝, 확정 프레임, 상태 해시)
  6. **브라우저 WebRTC P2P DataChannel 플레이어 (`axiom-wasm`)**: 24바이트 초경량 바이너리 패킷 통신, 32×32 협동 기지 렌더링, 플레이어 테두리 분리(P0 Sky Blue / P1 Orange), 0~300ms 지연/0~30% 드랍율 모의 네트워크 내장
  7. **전체 워크스페이스 단위 테스트 100% 통과**: 50개 단위 테스트 및 FFI 테스트 올패스

---

## 📜 과거 감사 기록 (Archived Audits)

### [AUDIT-20261022-v1.2.0] Axiom Engine v1.2.0 Universal Data-Driven Engine Architecture 종합 감사
- **일시:** 2026-10-22 10:55 KST
- **판정:** 🟢 **ALL CLEAR (v1.2.0 릴리즈 승인)**
- **검증:** 64B TypeDefinition/RecipeDefinition 슬랩, TypeRegistry/RecipeRegistry 무할당 O(1) 룩업, 64B TileCell 및 TileGrid<W, H>, 범용 C-ABI SDK(axiom-sdk), 팩토리 RPG 도메인 분리 실증 완료.

### [AUDIT-20261022-v1.1.0] Axiom Engine v1.1.0 Open-World Factory RPG 종합 마일스톤 감사
- **일시:** 2026-10-22 10:15 KST
- **판정:** 🟢 **ALL CLEAR (v1.1.0 릴리즈 승인)**
- **검증:** 무한 청크 물류, 10만 NPC 노동력, 거점 방어 A* 길찾기(Mazing), 웹 하드웨어 더블 버퍼링, GitHub Pages 라이브 배포 완료.

---

## 🛠️ 현장 시공 처방전 (Active Task Prescription)

### [RECIPE-20261022-T054] 1인 인디 개발자를 위한 네이티브 데스크톱 윈도우 러너 (`minifb`)
- **목표:** 웹 브라우저를 벗어나, `cargo run -p axiom-cli` 단 한 줄로 Windows 데스크톱에 60/144 FPS 인터랙티브 게임 창을 즉시 띄우고 실시간 마우스/키보드로 팩토리 디펜스 게임플레이를 조작할 수 있는 환경 완성.
- **대상 파일:**
  1. `crates/axiom-cli/Cargo.toml`:
     - `minifb = "0.29.0"` 의존성 추가
     - `axiom-render = { path = "../axiom-render" }` 추가
     - `axiom-game = { path = "../axiom-game" }` 추가
  2. `crates/axiom-cli/src/desktop.rs` (신규 파일):
     - `DesktopEngine`: 640x480 `FrameBuffer`, `FactoryGrid<32, 32>`, `WaveDirector<64>`, `BaseHeart`, 탄약 포탑 및 몬스터 전투 루프 탑재
     - `minifb::Window::new("Axiom Native Engine v1.3.0 - Desktop Studio", 640, 480, WindowOptions)`
     - 60 FPS 고정 틱 루프: WASD 이동, R키 회전, 1~8 키 건설 도구 선택, 마우스 좌클릭 배치, 마우스 우클릭 철거
     - ESC 키 입력 시 깔끔한 종료
  3. `crates/axiom-cli/src/main.rs`:
     - CLI 인자 검사: `--benchmark` 또는 `--headless` 플래그가 있으면 기존 벤치마크 및 리플레이어 실행
     - 인자가 없거나 `--window` 플래그 시 `desktop::run_desktop_window()` 실행
- **불변식 및 헌법 규정:**
  - 코어 로직 부동소수점 배제 유지 (`tools/guard.ps1` 무결성 검증 통과 필수)
  - 헤드리스 모드(`--benchmark`) 완벽 보존 (CI 및 자동화 테스트 깨짐 방지)
- **외부 러너 검증:** `powershell -ExecutionPolicy Bypass -File tools/guard.ps1`

---

## 🛠️ 긴급 처방전 템플릿 (Disaster Recovery Protocol)

```markdown
### [AUDIT-YYYYMMDD-XX] 이슈 제목
- **원인 분석:** (관제탑에서 분석한 핵심 원인 3줄 요약)
- **대상 파일:** `crates/.../foo.rs#L10-L25`
- **Claude 시공 지시사항:**
  1. ...
  2. ...
- **외부 러너 검증:** `.\tools\run_test.ps1` 실행 후 `tmp/verdict.txt` 확인
```

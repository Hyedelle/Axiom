# Axiom Architecture & Audit Report (docs/audit-report.md)

> **작성 주체:** 🏛️ Antigravity (Google Gemini - 총괄 관제탑 & 선행 연구소)  
> **수신 주체:** ⚡ Claude Code Max (현장 특수 시공팀) & Hyedelle (디렉터)  
> **목적:** 수만 줄 빌드 에러 로그, 아키텍처 결함, L1 캐시 위배 사항을 선행 분석하여 Claude Code가 10초 만에 수정할 수 있는 정밀 처방전 제공.

---

## 📌 최신 감사 요약 (Latest Audit)

- **일시:** 2026-10-22 14:20 KST
- **감사 대상:** Axiom Engine v1.5.0 상용 출시 네이티브 게임 루프, 정수배 스케일러 & 0-의존성 오디오 릴리즈 감사 (`axiom-cli`, `axiom-render`, `axiom-game`, `axiom-core`)
- **판정:** 🟢 **ALL CLEAR — 100% 헌법 및 무부동소수점 무결성, 144Hz 디커플링 누적기 타임스텝, 2x 정수배 픽셀 스케일러, 22kHz PCM 절차적 오디오 통과 (v1.5.0 공식 승인)**
- **주요 검증 항목:**
  1. **부동소수점(`f32`, `f64`) 원천 배제**: `axiom-core`, `axiom-sdk`, `axiom-game`, `axiom-render`, `axiom-cli` 전체 코드베이스 0건 (`tools/guard.ps1` 통과)
  2. **Glenn Fiedler 'Fix Your Timestep' 누적기 루프**: OS Sleep 지터 완전 박멸, 60Hz 불변 틱(UPS)과 144Hz/240Hz 디스플레이 주사율(FPS) 분리
  3. **무손실 정수배 픽셀 스케일러 (`upscale_nearest_2x`)**: 1x (704x544), 2x (1408x1088), F11 전체화면 종횡비 보존 레터박스 모드 지원
  4. **0-의존성 순수 정수 PCM 절차적 오디오 (`sound.rs`)**: 22,050Hz 16-bit Mono PCM 파형 합성(사각파 스윕, 삼각파, 노이즈) 및 Windows `PlaySoundW` 비동기 재생 연동 (포탑 사격, 몬스터 피격, 건설/철거, 퀵세이브 차임, 웨이브 알람 6종)
  5. **M키 음소거 토글**: 원클릭 오디오 음소거 및 HUD 비주얼 알림
  6. **헤드리스 벤치마크 및 CI 완전 호환**: `--benchmark` 플래그를 통한 100% 비트 일치 결정론 리플레이어 및 밸런싱 시뮬레이터 보존
  7. **전체 워크스페이스 단위 테스트 100% 통과**: 54개 단위 테스트 및 FFI 테스트 올패스

---

## 📜 과거 감사 기록 (Archived Audits)

### [AUDIT-20261022-v1.4.0] Axiom Engine v1.4.0 1인 인디 개발자 실사용 네이티브 스튜디오 및 픽셀 스프라이트 릴리즈 감사
- **일시:** 2026-10-22 13:30 KST
- **판정:** 🟢 **ALL CLEAR (v1.4.0 릴리즈 승인)**
- **검증:** 60 FPS 네이티브 윈도우, 무손실 F5/F9 퀵세이브, 16x16 스프라이트 아틀라스, F3 프로파일러 완료.

### [AUDIT-20261022-v1.3.0] Axiom Engine v1.3.0 WebRTC DataChannel & Rollback Netcode 종합 감사
- **일시:** 2026-10-22 12:20 KST
- **판정:** 🟢 **ALL CLEAR (v1.3.0 릴리즈 승인)**
- **검증:** 16B GridAction, 64-bit frame_tags 무한 링 버퍼, GridRollbackSession, WebRTC DataChannel 24B 바이너리 패킷 송수신 완료.

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

### [RECIPE-20261022-T063] 빌드 타임 바이너리 에셋 파이프라인 (`build.rs` & `.atlas` 슬랩) [완료]
- **목표:** Rust 소스 코드(`sprites.rs`) 안에 1,470줄로 하드코딩된 비트맵 데이터를 외부 에셋 정의로 분리하고, `build.rs`를 통해 컴파일 타임에 64B 정렬 고정 슬랩 바이너리(`sprites.atlas`)로 패킹하여 `include_bytes!`로 제로-카피 로드하는 상용 에셋 파이프라인 구축.
- **대상 파일:**
  1. `crates/axiom-render/build.rs` (신규 파일):
     - `crates/axiom-render/assets/sprites/` 디렉터리 내 에셋 정의를 순회/파싱
     - 64B 정렬 `AtlasHeader`: 매직 `0x41544C415358414D` ("AXMATLAS"), 버전, 스프라이트 개수, FNV-1a 체크섬
     - 각 스프라이트를 정확히 256개 u32 (`1,024` 바이트 = 16개 64B 캐시 라인) 배열로 바이너리 직렬화
     - `$OUT_DIR/sprites.atlas` 파일로 출력
  2. `crates/axiom-render/Cargo.toml`:
     - `build = "build.rs"` 명시
  3. `crates/axiom-render/src/sprites.rs`:
     - 1,470줄의 하드코딩된 문자열 배열 제거
     - `static ATLAS_BYTES: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/sprites.atlas"));`
     - 정렬된 슬랩 인덱싱 함수 `get_sprite_by_index(idx: usize) -> &'static [u32; 256]`
     - 기존 `get_animated_sprite(kind, dir, tick)` 및 `render_structure_sprite` 인터페이스 100% 호환 유지
  4. 신규 단위 테스트:
     - `crates/axiom-render/src/sprites.rs`: `test_baked_atlas_binary_integrity` (매직 헤더 및 256-word 스프라이트 슬랩 무결성 검증)
- **불변식 및 헌법 규정:**
  - 런타임 힙 할당 0, 부동소수점(`f32/f64`) 원천 배제
  - `tools/guard.ps1` 무결성 검증 100% 통과
- **외부 러너 검증:** `powershell -ExecutionPolicy Bypass -File tools/guard.ps1`

---

### [RECIPE-20261022-T061] 초경량 네이티브 오디오 출력 연동 (SFX: 발사, 채굴, 타격, 피격) [완료]
- **목표:** 타격감과 몰입감을 위해 외부 무거운 오디오 라이브러리 없이, 0 의존성 순수 절차적 PCM 신디사이저와 Windows 비동기 네이티브 사운드 출력(`winmm PlaySound`)을 연동.
- **대상 파일:**
  1. `crates/axiom-cli/src/sound.rs` (신규 파일):
     - `SoundKind`: `Shoot` (포탑 사격), `Hit` (몬스터 타격), `Build` (구조물 배치), `Demolish` (구조물 철거), `SaveChime` (F5 퀵세이브), `WaveAlarm` (웨이브 경보)
     - 22,050Hz 16-bit Mono PCM 절차적 파형 생성기 (사각파 스윕, 삼각파, 노이즈 버스트, 0 Float 정수 연산)
     - 44B RIFF/WAVE 헤더 패커 (`generate_wav_bytes`)
     - `play_sound(kind: SoundKind)`:
       - Windows: `PlaySoundW` 비동기 메모리 버퍼 재생 (`SND_MEMORY | SND_ASYNC | SND_NODEFAULT`)
       - Non-Windows / Headless: no-op
     - `is_muted: bool` 및 `toggle_mute()` 지원
  2. `crates/axiom-cli/src/desktop.rs`:
     - `sound` 모듈 연동 및 게임플레이 이벤트별 사운드 트리거:
       - 포탑 사격 시 `SoundKind::Shoot`
       - 몬스터 피격 시 `SoundKind::Hit`
       - 건설 배치 시 `SoundKind::Build`
       - 철거 시 `SoundKind::Demolish`
       - F5 퀵세이브 시 `SoundKind::SaveChime`
       - 새 웨이브 스폰 시 `SoundKind::WaveAlarm`
       - `M` 키: 음소거(Mute) 토글 및 사이드바 범례 `M:MUTE` 추가
  3. `crates/axiom-cli/src/lib.rs`:
     - `pub mod sound;`
  4. 신규 단위 테스트:
     - `crates/axiom-cli/src/sound.rs`: `test_wav_header_and_pcm_generation`
- **불변식 및 헌법 규정:**
  - 부동소수점(`f32/f64`) 원천 배제, 순수 정수 파형 합성
  - `tools/guard.ps1` 무결성 검증 100% 통과
- **외부 러너 검증:** `powershell -ExecutionPolicy Bypass -File tools/guard.ps1`

---

### [RECIPE-20261022-T060] 무손실 정수배 픽셀 스케일러 & 전체화면 모드 (1x/2x/F11) [완료]
- **목표:** QHD/4K 고해상도 모니터에서 레트로 픽셀 아트의 흐림/왜곡 없는 무손실 정수배(Integer Scaling 1x/2x) 래스터라이징 및 F11 전체화면 토글 지원.
- **대상 파일:**
  1. `crates/axiom-render/src/framebuffer.rs`:
     - `upscale_nearest_2x(src: &[u32], src_w: usize, src_h: usize, dst: &mut [u32])`:
       - 1픽셀을 2x2 정수 블록으로 고속 복제 (순수 정수 연산, 0 float)
       - `dst` 크기: `(src_w * 2) * (src_h * 2)`
  2. `crates/axiom-cli/src/desktop.rs`:
     - 창 크기 및 스케일 상태 관리 (`ScaleMode: 1x, 2x, Fullscreen`)
     - `F10` 키: 1x (704x544) ↔ 2x (1408x1088) 스케일 토글
     - `F11` 키: 전체화면(Fullscreen) 토글
     - 사이드바 범례에 `F10:SCL`, `F11:FULL` 추가
  3. 신규 단위 테스트:
     - `crates/axiom-render/src/framebuffer.rs`: `test_upscale_nearest_2x_exactness`
- **불변식 및 헌법 규정:**
  - 부동소수점(`f32/f64`) 원천 배제, 순수 정수/비트 연산
  - `tools/guard.ps1` 무결성 검증 100% 통과
- **외부 러너 검증:** `powershell -ExecutionPolicy Bypass -File tools/guard.ps1`

---

### [RECIPE-20261022-T059] Glenn Fiedler 'Fix Your Timestep' 누적기 루프 (`crates/axiom-cli/src/desktop.rs`) [완료]
- **목표:** Windows OS Sleep Jitter 및 144Hz/240Hz 게이밍 모니터에서 발생하는 마이크로 스터터링(저더)을 완전히 박멸하고, 60Hz 불변 틱 시뮬레이션과 모니터 주사율 렌더링을 완전히 분리하는 누적기(Accumulator) 루프 구축.
- **대상 파일:**
  1. `crates/axiom-cli/src/desktop.rs`:
     - 상수 정의:
       - `TICK_RATE_HZ: u64 = 60`
       - `DT_FIXED: Duration = Duration::from_nanos(1_000_000_000 / 60)` (~16.666ms)
       - `MAX_ACCUMULATOR: Duration = Duration::from_millis(100)` (나선형 시간 지연 클램핑)
     - `run_desktop_window()` 메인 루프 개편:
       - 고정 딜레이 슬립 의존 제거 (`limit_update_rate` 대신 VSync 또는 무제한 가변 주사율 루프)
       - `previous_time` 및 `accumulator: Duration` 누적 관리
       - `while accumulator >= DT_FIXED { engine.step(); accumulator -= DT_FIXED; }`
       - 프레임버퍼 렌더링은 매 디스플레이 프레임마다 1회 수행
       - `ProfilerMetrics`에 `ups`(Updates Per Second, 초당 틱 갱신 수) 및 `fps` 분리 계측 탑재
  2. 신규 단위 테스트:
     - `crates/axiom-cli/src/desktop.rs`: `test_timestep_accumulator_logic` (가변 델타 타임 인가 시 정확한 60Hz 틱 횟수 보장 검증)
- **불변식 및 헌법 규정:**
  - 부동소수점(`f32/f64`) 원천 배제, 순수 정수/Duration 시간 연산
  - `tools/guard.ps1` 무결성 검증 100% 통과
- **외부 러너 검증:** `powershell -ExecutionPolicy Bypass -File tools/guard.ps1`

---

### [RECIPE-20261022-T057] 1인 개발자를 위한 실시간 인게임 프로파일러 & 디버그 오버레이 (F3) [완료]
- **목표:** 1인 인디 개발자가 게임플레이 중 즉각적인 성능 병목과 시스템 상태를 진단할 수 있도록, F3 핫키 토글 인게임 실시간 프로파일러 HUD 오버레이 구축.
- **대상 파일:**
  1. `crates/axiom-cli/src/desktop.rs`:
     - `ProfilerMetrics`:
       - `sim_time_us: u64`: `engine.step()` 소요 마이크로초
       - `render_time_us: u64`: 래스터라이징 소요 마이크로초
       - `fps: u32`: 실시간 FPS
       - `active_structures: usize`: 활성 타일/구조물 수
       - `active_monsters: usize`: 생존 몬스터 수
       - `active_items: usize`: 벨트 및 인벤토리 아이템 수
       - `state_hash: u64`: FNV-1a 결정론 해시
       - `frame_times: [u32; 60]`: 최근 60프레임 프레임 타임 링 버퍼
     - `show_profiler: bool` 상태 및 `minifb::Key::F3` 토글 처리
     - `render_profiler_overlay(&self, fb: &mut FrameBuffer)`:
       - 반투명 어두운 배경 패널 (`0xDD0F172A`)
       - 실시간 텍스트 수치 (Sim, Render, FPS, Entities, Hash)
       - 60프레임 미니 스파크라인 바 차트 (기준선 16.6ms 대비 실측 막대)
     - 사이드바 CONTROLS 범례에 `F3:PROF` 추가
  2. 신규 단위 테스트:
     - `crates/axiom-cli/src/desktop.rs`: `test_desktop_profiler_metrics_collection`
- **불변식 및 헌법 규정:**
  - 부동소수점(`f32/f64`) 원천 배제, 순수 정수 연산
  - `tools/guard.ps1` 무결성 검증 100% 통과
- **외부 러너 검증:** `powershell -ExecutionPolicy Bypass -File tools/guard.ps1`

---

### [RECIPE-20261022-T056] 16x16 픽셀 스프라이트 아틀라스 및 애니메이션 래스터라이저 (`crates/axiom-render`) [완료]
- **목표:** 단색 사각형 색칠을 탈피하여, 1인 인디 개발자의 상용 비주얼을 위해 16x16 결정론적 픽셀 스프라이트 아틀라스와 실시간 애니메이션(벨트 흐름, 드릴 회전, 제련소 불꽃, 하트 펄스, 포탑, 몬스터) 래스터라이저 구축.
- **대상 파일:**
  1. `crates/axiom-render/src/sprites.rs` (신규 모듈):
     - 16x16 픽셀 슬랩 (`[u32; 256]` ARGB):
       - `BELT_FRAMES`: 4프레임 방향별 벨트 흐름 애니메이션
       - `DRILL_FRAMES`: 4프레임 회전 채굴기 스프라이트
       - `SMELTER_FRAMES`: 4프레임 타오르는 제련소 불꽃 애니메이션
       - `CHEST_SPRITE`: 리벳 보강된 메탈릭 수송 상자
       - `WALL_SPRITE`: 성벽 석재 블록
       - `TURRET_SPRITE`: 방향별 포신 및 거치대
       - `HEART_FRAMES`: 4프레임 펄스 에너지 크리스탈 코어
       - `MONSTER_FRAMES`: 2프레임 걷는 고블린 크리처
       - `ORE_DEPOSIT`: 바위 틈의 철/구리 광석 비트맵
     - `get_animated_sprite(kind, direction, tick) -> &'static [u32; 256]`
  2. `crates/axiom-render/src/framebuffer.rs`:
     - `draw_sprite_16x16(&mut self, x: i32, y: i32, sprite: &[u32; 256])`: 알파 키(`(color >> 24) > 0`) 검사 및 화면 경계 클리핑
     - `draw_sprite_16x16_tinted(&mut self, x: i32, y: i32, sprite: &[u32; 256], tint: u32)`
  3. `crates/axiom-render/src/lib.rs`:
     - `pub mod sprites;`
     - `pub use sprites::*;`
  4. `crates/axiom-cli/src/desktop.rs`:
     - 구조물 및 엔티티 렌더링 시 단색 `draw_rect` 대신 `draw_sprite_16x16` 스프라이트 아틀라스 호출로 전환
  5. 신규 단위 테스트:
     - `crates/axiom-render/src/sprites.rs`: `test_sprite_dimensions_and_alpha_integrity`
- **불변식 및 헌법 규정:**
  - 부동소수점(`f32/f64`) 원천 배제, 순수 정수 비트맵 슬랩
  - `tools/guard.ps1` 무결성 검증 100% 통과
- **외부 러너 검증:** `powershell -ExecutionPolicy Bypass -File tools/guard.ps1`

---

### [RECIPE-20261022-T055] 1인 인디 개발자를 위한 디스크 세이브/로드(F5/F9) 및 영속성 슬롯 (`saves/*.axiom`) [완료]
- **목표:** 1인 인디 개발자가 데스크톱에서 기지를 건설하고 웨이브를 막아낸 상태를 F5 퀵세이브로 디스크 파일(`saves/quick.axiom`)에 100% 무손실 저장하고, F9 퀵로드로 1초 만에 그대로 복원하여 게임플레이를 연속 검증할 수 있는 파일 영속성 구축.
- **대상 파일:**
  1. `crates/axiom-cli/src/desktop.rs`:
     - `SaveFileHeader`: 64B 정렬 매직 헤더 (`0x4D4F495841564153`), 버전 `0x00010300`, 틱, 베이스 하트 HP, 웨이브, 총 제련 수, FNV-1a 해시
     - `DesktopEngine::save_to_file(&self, path: &str) -> std::io::Result<()>`:
       - `saves/` 디렉터리 자동 생성 (`std::fs::create_dir_all("saves")`)
       - `grid` 32x32 타일 데이터, 하트 HP, 포탑 탄약, 몬스터 배열, 웨이브 정보 바이너리 패킹 및 파일 기록
       - 저장 성공 시 HUD에 `[QUICK SAVE: saves/quick.axiom]` 안내 메시지 트리거
     - `DesktopEngine::load_from_file(&mut self, path: &str) -> std::io::Result<()>`:
       - 파일 헤더 매직 넘버 및 체크섬 검증
       - 타일 그리드, 웨이브, 하트 HP, 몬스터 상태 무손실 복원
       - 로드 성공 시 HUD에 `[QUICK LOAD COMPLETED (Tick XXX)]` 안내 메시지 트리거
     - 키보드 F5 (퀵세이브), F9 (퀵로드) 키 이벤트 바인딩 (`minifb::Key::F5`, `minifb::Key::F9`)
  2. 신규 단위 테스트 추가:
     - `crates/axiom-cli/src/desktop.rs`: `test_desktop_save_load_roundtrip` (임의의 기지 배치 후 저장 ➔ 복원 후 상태 비트 일치 검증)
- **불변식 및 헌법 규정:**
  - 부동소수점(`f32/f64`) 원천 배제 및 비트 일치 결정론적 바이너리 직렬화
  - `tools/guard.ps1` 무결성 검증 100% 통과
- **외부 러너 검증:** `powershell -ExecutionPolicy Bypass -File tools/guard.ps1`

---

### [RECIPE-20261022-T054] 1인 인디 개발자를 위한 네이티브 데스크톱 윈도우 러너 (`minifb`) [완료]
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

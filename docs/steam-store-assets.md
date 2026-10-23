# 🎮 Mimic The Show — Official Steam Store Assets & Page Specification

> **Axiom 120-Task Milestone Release (Engine v2.1.0 / Game v1.0.0)**  
> **Target Platform:** Steam (Windows x64 / SteamOS / Steam Deck Verified)  
> **Engine Architecture:** 100% Deterministic Pure Integer Math (0 Floats) · 64B L1 Cache DOD  

---

## 1. Store Page Identity & Metadata

* **Game Title:** Mimic The Show (미믹 더 쇼: 스틸 디사이더)
* **Tagline:** 100% Deterministic Punch-Out Mecha Boxing Roguelite
* **Developer / Publisher:** Hyedelle / Dreamhub Engine
* **Supported Languages:** English, Korean (한국어)
* **Steam Deck Compatibility:** **VERIFIED** (16:10 1280x800 native scaling, full gamepad controller prompts, 60 FPS rock-solid, <4W battery draw)
* **Steamworks Features:**
  * Steam Achievements (12 Achievements with 64-bit anti-tamper checksums)
  * Steam Cloud (Crash-free atomic save sync)
  * Steam Workshop (One-click modding, `.axmod` packaging, GLB parts & decals)
  * Steam Leaderboards (Integer ELO ranking & Grandmaster tiers)
  * Steam P2P Lobby (8-member multiplayer match broker & spectator relay)

---

## 2. Store Descriptions

### 📄 Short Description (Short Summary - 300 Chars)
> **EN:** Step into the steel ring in *Mimic The Show*, a lightning-fast mecha boxing roguelite running on a pure integer 3D software rasterizer! Read your opponent's telegraphs, unleash devastating Nova Finishers, customize your frame with Steam Workshop parts, and climb the global ELO ladder.

> **KO:** 0-Float 100% 절대 결정론 소프트웨어 래스터라이저로 구동되는 하이퍼 스피드 메카 복싱 로그라이트! 상대의 텔레그래프를 간파하여 벼락같은 회피와 노바 피니셔를 꽂아넣고, 스팀 워크숍 부품으로 나만의 기체를 빌드하며 글로벌 ELO 랭킹의 정점에 오르십시오.

### 📝 Detailed Description (About This Game)

```markdown
### 🥊 PURE REFLEX & STEEL PRECISION: PUNCH-OUT MECHA BOXING
*Mimic The Show* strips away modern bloated physics in favor of 100% deterministic, 
frame-perfect combat. Every punch, weave, guard, and counter-strike executes with 
sub-millisecond responsiveness.

* **5 High-Impact Combat Actions:** Lead Jab, Heavy Cross, Whipping Hook, Peek-a-boo Guard, and Directional Slip Weaves.
* **Cinematic Nova Finisher:** Fill your Overdrive gauge to unleash a 4-phase cinematic sequence—core overcharging, screen-tearing impacts, 40-spark debris explosions, and low-angle champion victory roars!
* **Dynamic 3D Software Rasterizer:** Built from scratch in pure Rust. Features Juan Pineda 8x8 tile binning, Reversed-Z sub-pixel depth precision, inverted-hull ink outlines, and incandescent rim lighting at blazing 870+ FPS.

### 🔧 INBOUND DRAFTS & DEEP GARAGE MODULARITY
* **Procedural Assembly:** Swap Head, Core, Arm, and Leg components between rounds to trigger passive set synergies and Overclock perks.
* **Steam Workshop (.axmod):** Create custom 3D mecha parts and decal skins in the integrated Painter Studio, or subscribe to community creations with 1-click runtime hot-swapping!
* **Global ELO & 8-Player Lobby:** Challenge rivals worldwide via WebRTC & Steamworks P2P rollback netcode with zero-allocation insertion-sorted leaderboards.
```

---

## 3. Steam Capsule Artwork Specifications

| Asset Type | Dimensions | Aspect Ratio | Format | File Name | Purpose |
| :--- | :--- | :--- | :--- | :--- | :--- |
| **Header Capsule** | **460 × 215 px** | ~2.14:1 | PNG / JPG | `capsule_header.png` | Main store search results & discovery queue |
| **Small Capsule** | **231 × 87 px** | ~2.65:1 | PNG / JPG | `capsule_small.png` | Sidebar promos, wishlist emails, specials |
| **Main Capsule** | **616 × 353 px** | ~1.74:1 | PNG / JPG | `capsule_main.png` | Front page feature carousel & daily deals |
| **Vertical Capsule**| **374 × 448 px** | ~1:1.20 | PNG / JPG | `capsule_vertical.png`| Steam Library grid cover & collection cards |
| **Page Background**| **1438 × 810 px** | 16:9 | PNG / JPG | `page_bg.png` | Store page atmosphere & backdrop |

### 🎨 Art Direction Guidelines for Capsules
1. **Focal Point:** ELPIS mecha delivering a Nova Finisher punch directly toward the viewer, fists crackling with white/gold energy arcs.
2. **Foreground Title:** Bold metallic brass logo *"MIMIC THE SHOW"* with rivet studs and steam venting exhausts.
3. **Background Ring:** Neon arena floodlights, crowd silhouette cheering, flying 3D golden debris sparks.
4. **Contrast & Legibility:** High-contrast rim lighting ensures the silhouette pops even at thumbnail sizes (231x87).

---

## 4. Minimum & Recommended System Requirements

```ini
[Minimum System Requirements]
OS: Windows 10 / 11 64-bit (or SteamOS 3.0+ / Linux via Proton)
Processor: Dual Core CPU @ 1.8 GHz (x86_64, SSE2 compatible)
Memory: 512 MB RAM
Graphics: Any integrated or dedicated GPU supporting 2D window blit (OpenGL 2.1 / GDI / Software)
DirectX: Not Required
Storage: 50 MB available space
Sound Card: Windows Compatible Stereo Sound (Pure integer procedural PCM synthesis)
Additional Notes: Runs natively on CPU L1/L2 cache; zero floating-point nondeterminism.

[Recommended System Requirements]
OS: Windows 11 64-bit or SteamOS
Processor: Quad Core CPU @ 2.5 GHz
Memory: 1 GB RAM
Graphics: Any display with 60 Hz or higher refresh rate
Network: Broadband Internet connection (for P2P multiplayer & global ELO leaderboards)
Storage: 100 MB available space (including Workshop mods & recorded .axrp replays)
Target Performance: Constant 60.0 FPS @ 16.6ms / Sub-1ms CPU Frame Time
```

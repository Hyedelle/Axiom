//! Native Desktop Boxing View for Axiom Engine (Track 2: Mecha Boxing).
//!
//! Provides a real-time 60 FPS interactive Punch-Out boxing match within the `minifb` window.
//! Features pure integer cel-shaded robot boxers, hit-stop, camera shake,
//! 2D anime cut-in finisher overlays, and sound synthesis.

use minifb::{Key, Window};
use std::cell::RefCell;
use std::path::Path;

use axiom_core::Fixed;
use axiom_game::boxing::{ActionKind, AiPhase, BoxingEvent, BoxingRingEngine, FightPhase};
use axiom_game::card::DeckState;
use axiom_render::camera3d::Camera3D;
use axiom_render::cutin::{render_cutin_overlay, CutinKind};
use axiom_render::kinematics3d::{evaluate_forward_kinematics, MechaPose3D};
use axiom_render::math3d::{Mat4Fixed, Vec3Fixed};
use axiom_render::mesh3d::{parse_glb, MechaModel3D};
use axiom_render::particles3d::ParticleSystem3D;
use axiom_render::raster3d::Rasterizer3D;
use axiom_render::FrameBuffer;

use crate::sound::{self, SoundKind};

/// Boxing View state for desktop execution.
pub struct BoxingView {
    pub engine: BoxingRingEngine,
    pub deck: DeckState<16>,
    pub cutin_frames: u16,
    pub cutin_kind: CutinKind,
    pub feedback_timer: u16,
    pub feedback_msg: &'static str,
    pub feedback_color: u32,
    pub rng_state: u64,
    pub hero_model: Option<MechaModel3D>,
    pub opponent_model: Option<MechaModel3D>,
    pub rasterizer: RefCell<Rasterizer3D>,
    pub camera: Camera3D,
    pub particles: RefCell<ParticleSystem3D<128>>,
    pub use_3d: bool,
}

impl BoxingView {
    pub fn new(seed: u64) -> Self {
        let hero_glb_path = Path::new(r"C:\Dreamhub\Game\steel-the-show\assets\robot3d\hero\hero_a.glb");
        let hero_model = if hero_glb_path.exists() {
            std::fs::read(hero_glb_path)
                .ok()
                .and_then(|bytes| parse_glb(&bytes).ok())
        } else {
            None
        };

        // Opponent candidate DUMMY_v2 or fallback to opponents/DUMMY.glb
        let opp_path_v2 = Path::new(r"C:\Dreamhub\Game\steel-the-show\assets\robot3d\opponent_candidates\DUMMY_v2\DUMMY.glb");
        let opp_path_std = Path::new(r"C:\Dreamhub\Game\steel-the-show\assets\robot3d\opponents\DUMMY.glb");
        let opp_path = if opp_path_v2.exists() {
            opp_path_v2
        } else {
            opp_path_std
        };

        let opponent_model = if opp_path.exists() {
            std::fs::read(opp_path)
                .ok()
                .and_then(|bytes| parse_glb(&bytes).ok())
        } else {
            None
        };

        let has_model = hero_model.is_some();
        let r = Rasterizer3D::new(704, 544);

        Self {
            engine: BoxingRingEngine::new(seed),
            deck: DeckState::new(),
            cutin_frames: 0,
            cutin_kind: CutinKind::OverheatMeltdown,
            feedback_timer: 0,
            feedback_msg: "FIGHT START! [A/D: SLIP, S: GUARD, J/K/L: PUNCH, F4: 3D/2D]",
            feedback_color: 0xFFFBBF24,
            rng_state: seed.wrapping_add(12345),
            hero_model,
            opponent_model,
            rasterizer: RefCell::new(r),
            camera: Camera3D::default(),
            particles: RefCell::new(ParticleSystem3D::new(seed.wrapping_add(98765))),
            use_3d: has_model,
        }
    }

    /// Advances the boxing simulation by 1 frame based on keyboard input.
    pub fn step(&mut self, window: &Window) {
        let mut input = ActionKind::Idle;

        if self.engine.fight_phase == FightPhase::Drafting {
            if window.is_key_pressed(Key::Key1, minifb::KeyRepeat::No)
                || window.is_key_pressed(Key::NumPad1, minifb::KeyRepeat::No)
                || window.is_key_pressed(Key::J, minifb::KeyRepeat::No)
            {
                input = ActionKind::Jab; // Picks card 0
                sound::play_sound(SoundKind::Build);
            } else if window.is_key_pressed(Key::Key2, minifb::KeyRepeat::No)
                || window.is_key_pressed(Key::NumPad2, minifb::KeyRepeat::No)
                || window.is_key_pressed(Key::K, minifb::KeyRepeat::No)
            {
                input = ActionKind::Cross; // Picks card 1
                sound::play_sound(SoundKind::Build);
            } else if window.is_key_pressed(Key::Key3, minifb::KeyRepeat::No)
                || window.is_key_pressed(Key::NumPad3, minifb::KeyRepeat::No)
                || window.is_key_pressed(Key::L, minifb::KeyRepeat::No)
            {
                input = ActionKind::Hook; // Picks card 2
                sound::play_sound(SoundKind::Build);
            }
        } else if self.engine.fight_phase == FightPhase::Defeat || self.engine.fight_phase == FightPhase::Victory {
            if window.is_key_pressed(Key::Space, minifb::KeyRepeat::No) || window.is_key_pressed(Key::Enter, minifb::KeyRepeat::No) {
                input = ActionKind::Finisher; // Resets run
                sound::play_sound(SoundKind::SaveChime);
            }
        } else {
            // Normal combat controls
            if window.is_key_down(Key::Space) {
                input = ActionKind::Finisher;
                if self.cutin_frames == 0 && self.engine.player.heat <= self.engine.player.max_heat {
                    self.cutin_frames = 40; // ~666ms anime cutin
                    self.cutin_kind = CutinKind::OverheatMeltdown;
                    sound::play_sound(SoundKind::Hit);
                }
            } else if window.is_key_down(Key::S) || window.is_key_down(Key::Down) {
                input = ActionKind::Guard;
            } else if window.is_key_pressed(Key::A, minifb::KeyRepeat::No)
                || window.is_key_pressed(Key::Left, minifb::KeyRepeat::No)
            {
                input = ActionKind::SlipLeft;
                sound::play_sound(SoundKind::Shoot);
            } else if window.is_key_pressed(Key::D, minifb::KeyRepeat::No)
                || window.is_key_pressed(Key::Right, minifb::KeyRepeat::No)
            {
                input = ActionKind::SlipRight;
                sound::play_sound(SoundKind::Shoot);
            } else if window.is_key_pressed(Key::J, minifb::KeyRepeat::No)
                || window.is_key_pressed(Key::Z, minifb::KeyRepeat::No)
            {
                input = ActionKind::Jab;
                sound::play_sound(SoundKind::Shoot);
            } else if window.is_key_pressed(Key::K, minifb::KeyRepeat::No)
                || window.is_key_pressed(Key::X, minifb::KeyRepeat::No)
            {
                input = ActionKind::Cross;
                sound::play_sound(SoundKind::Shoot);
            } else if window.is_key_pressed(Key::L, minifb::KeyRepeat::No)
                || window.is_key_pressed(Key::C, minifb::KeyRepeat::No)
            {
                input = ActionKind::Hook;
                sound::play_sound(SoundKind::Shoot);
            }
        }

        if window.is_key_pressed(Key::F4, minifb::KeyRepeat::No) {
            self.use_3d = !self.use_3d;
            let msg = if self.use_3d {
                "3D MECHA RASTERIZER ON [PINEDA 8x8 L1]"
            } else {
                "2D RETRO SPRITE MODE ON"
            };
            self.set_feedback(msg, 0xFF38BDF8);
        }

        // Advance simulation
        let event = self.engine.step(input);

        // Step 3D camera and particle kinematics
        self.camera.step_kinematics();
        self.particles.borrow_mut().step_kinematics();

        // Process audio & visual events
        match event {
            BoxingEvent::PlayerPunched { damage: _, is_counter } => {
                sound::play_sound(SoundKind::Hit);
                self.camera.trigger_punch_zoom(Fixed::from_raw((0.20 * 65536.0) as i64));
                self.particles.borrow_mut().spawn_sparks(Vec3Fixed::from_f32(0.0, 0.1, 0.4), 16);
                if is_counter {
                    self.set_feedback("COUNTER CRITICAL! [150% DMG]", 0xFFF97316);
                } else {
                    self.set_feedback("PUNCH HIT!", 0xFFFBBF24);
                }
            }
            BoxingEvent::PlayerEvaded => {
                sound::play_sound(SoundKind::Shoot);
                self.particles.borrow_mut().spawn_steam(Vec3Fixed::from_f32(0.0, -0.1, -0.3), 8);
                self.set_feedback("SLIP EVADED! COUNTER NOW!", 0xFF38BDF8);
            }
            BoxingEvent::PlayerBlocked { chip: _ } => {
                sound::play_sound(SoundKind::Build);
                self.set_feedback("BLOCKED! [CHIP DMG]", 0xFF34D399);
            }
            BoxingEvent::PlayerStruck { damage: _ } => {
                sound::play_sound(SoundKind::Demolish);
                self.set_feedback("STRUCK BY OPPONENT!", 0xFFEF4444);
            }
            BoxingEvent::OpponentDowned => {
                sound::play_sound(SoundKind::SaveChime);
                self.set_feedback("KNOCK OUT! VICTORY!", 0xFFEAB308);
                self.cutin_frames = 60;
                self.cutin_kind = CutinKind::Knockout;
            }
            BoxingEvent::PlayerDowned => {
                sound::play_sound(SoundKind::Demolish);
                self.set_feedback("PLAYER DOWNED! MATCH OVER", 0xFFDC2626);
            }
            BoxingEvent::DraftOpened => {
                sound::play_sound(SoundKind::SaveChime);
                self.set_feedback("STAGE CLEARED! SELECT TACTICAL CARD [1, 2, OR 3]", 0xFFFBBF24);
            }
            BoxingEvent::CardDrafted { kind: _ } => {
                sound::play_sound(SoundKind::Build);
                self.set_feedback("CARD EQUIPPED! ADVANCING TO NEXT ARENA...", 0xFF38BDF8);
            }
            BoxingEvent::StageCleared { next_stage: _ } => {
                sound::play_sound(SoundKind::SaveChime);
                self.set_feedback("NEXT OPPONENT ENGAGING!", 0xFFF97316);
            }
            BoxingEvent::None => {}
        }

        if self.feedback_timer > 0 {
            self.feedback_timer -= 1;
        }
        if self.cutin_frames > 0 {
            self.cutin_frames -= 1;
        }
    }

    fn set_feedback(&mut self, msg: &'static str, color: u32) {
        self.feedback_msg = msg;
        self.feedback_color = color;
        self.feedback_timer = 60; // 1.0s
    }

    /// Renders the complete boxing match onto the 704x544 FrameBuffer.
    pub fn render(&self, fb: &mut FrameBuffer) {
        let w = fb.width as i32;
        let h = fb.height as i32;

        // Camera Shake offset computed deterministically from tick
        let s = self.engine.camera_shake as i32;
        let shake_x = if s > 0 { (((self.engine.tick as i32 & 0x7) - 4) * s) / 3 } else { 0 };
        let shake_y = if s > 0 { ((((self.engine.tick as i32 >> 3) & 0x7) - 4) * s) / 3 } else { 0 };

        // 1. Dark Arena Backdrop
        fb.clear(0xFF07090E);

        // 2. Boxing Ring Floor & Ropes
        let ring_cx = w / 2 + shake_x;
        let ring_cy = h / 2 + 50 + shake_y;

        // Ring canvas floor (Perspective trapezoid)
        for y in (ring_cy - 120)..(ring_cy + 120) {
            let dy = y - (ring_cy - 120);
            let half_span = 140 + (dy * 90) / 240;
            for x in (ring_cx - half_span)..(ring_cx + half_span) {
                let checker = ((x / 32) + (y / 16)) % 2 == 0;
                let col = if checker { 0xFF141923 } else { 0xFF181F2C };
                fb.set_pixel(x, y, col);
            }
            // Ring floor outer ink line
            fb.set_pixel(ring_cx - half_span, y, 0xFF0A0D14);
            fb.set_pixel(ring_cx + half_span, y, 0xFF0A0D14);
        }

        // Ring Ropes (3 colored horizontal bands)
        for &ry in &[ring_cy - 70, ring_cy - 30, ring_cy + 10] {
            fb.draw_rect(ring_cx - 220, ry, 440, 3, 0xFFDC2626); // Red rope
            fb.draw_rect(ring_cx - 220, ry + 1, 440, 1, 0xFFFCA5A5); // Highlight
        }

        // 3. Render Opponent Mecha Boxer (Center Top, Facing Down)
        if self.use_3d {
            self.rasterizer.borrow_mut().clear_depth();
        }

        let opp_base_x = ring_cx;
        let opp_base_y = ring_cy - 50;
        self.render_opponent_boxer(fb, opp_base_x, opp_base_y, w, h);

        // 4. Render Player Mecha Boxer (Foreground Bottom, Facing Up)
        let player_base_x = ring_cx + self.engine.player.x_offset_px as i32;
        let player_base_y = ring_cy + 110;
        self.render_player_boxer(fb, player_base_x, player_base_y, w, h);

        // 4.5. Render 3D Sparks & Steam Particles
        if self.use_3d {
            let mut r = self.rasterizer.borrow_mut();
            let cam_dist = self.camera.effective_distance();
            let focal_length = self.camera.effective_focal_length();
            self.particles.borrow().render(
                fb,
                &mut r.depth_buffer,
                &Mat4Fixed::IDENTITY,
                cam_dist,
                focal_length,
            );
        }

        // 5. Render HUD Elements
        self.render_hud(fb, w, h);

        // 6. 2D Anime Cut-in Overlay
        if self.cutin_frames > 0 {
            let permille = (40 - self.cutin_frames) as u32 * 1000 / 40;
            render_cutin_overlay(fb, self.cutin_kind, permille, ring_cx, ring_cy);
        }

        // 7. Tactical Overlays (Card Draft / Run Defeat)
        if self.engine.fight_phase == FightPhase::Drafting {
            self.render_draft_overlay(fb, w, h);
        } else if self.engine.fight_phase == FightPhase::Defeat {
            self.render_defeat_overlay(fb, w, h);
        }
    }

    fn render_opponent_boxer(&self, fb: &mut FrameBuffer, cx: i32, cy: i32, w: i32, _h: i32) {
        if self.use_3d {
            if let Some(model) = &self.opponent_model {
                let mut r = self.rasterizer.borrow_mut();

                // Opponent faces player (South, BAM 0)
                let base_rot = Mat4Fixed::from_rotation_y(0);

                // Opponent kinematics pose based on AI state:
                let opp_pose = if self.engine.opponent.action.kind == ActionKind::Down {
                    MechaPose3D::down()
                } else if self.engine.opponent.action.kind == ActionKind::HitStagger {
                    MechaPose3D::hit_stagger()
                } else if self.engine.ai_phase == AiPhase::Telegraph {
                    let mut p = MechaPose3D::default();
                    p.arm_r_offset = Vec3Fixed::from_f32(0.0, 0.1, -0.2);
                    p.torso_yaw_y = 12;
                    p
                } else if self.engine.opponent.action.kind == ActionKind::Jab {
                    MechaPose3D::jab(500)
                } else if self.engine.opponent.action.kind == ActionKind::Cross
                    || self.engine.opponent.action.kind == ActionKind::Hook
                {
                    MechaPose3D::cross(500)
                } else {
                    MechaPose3D::idle(self.engine.tick)
                };

                let transforms = evaluate_forward_kinematics(&opp_pose, true);

                // Positioned deeper into the ring (+0.75m Z)
                let trans_mat = Mat4Fixed::from_translation(Vec3Fixed::from_f32(
                    (cx - w / 2) as f32 * 0.003,
                    0.05,
                    0.75,
                ));

                let world_mat = base_rot.mul_mat(&trans_mat);
                let cam_dist = Fixed::from_raw((2.4 * 65536.0) as i64);
                let focal_length = 520;

                r.render_model_posed(fb, model, &transforms, &world_mat, cam_dist, focal_length);
                return;
            }
        }

        let is_telegraph = self.engine.ai_phase == AiPhase::Telegraph;
        let is_hit = self.engine.opponent.action.kind == ActionKind::HitStagger;
        let is_down = self.engine.opponent.action.kind == ActionKind::Down;

        let body_y = if is_down { cy + 40 } else { cy };
        let tint = if is_hit {
            0xFFFFFFFF // Hit flash white
        } else if is_telegraph {
            0xFFEF4444 // Red warning tint
        } else {
            self.engine.opponent_kind.armor_tint()
        };

        // Opponent Torso (Cel shaded with dark outline)
        fb.draw_rect(cx - 28, body_y - 48, 56, 48, 0xFF0F172A); // Ink Outline
        fb.draw_rect(cx - 25, body_y - 45, 50, 42, tint);       // Lit Armor
        fb.draw_rect(cx - 25, body_y - 25, 50, 22, 0xFF1E3A8A); // Shade Band

        // Opponent Head
        fb.draw_rect(cx - 16, body_y - 74, 32, 24, 0xFF0F172A); // Outline
        fb.draw_rect(cx - 14, body_y - 72, 28, 20, 0xFF64748B); // Metal Helm

        // Eyes (Visor)
        let eye_col = if is_telegraph { 0xFFFF0000 } else { 0xFF00FFCC };
        fb.draw_rect(cx - 10, body_y - 66, 20, 6, eye_col);

        // Opponent Fists
        if !is_down {
            let l_punch = if self.engine.opponent.action.kind == ActionKind::Jab { 20 } else { 0 };
            let r_punch = if self.engine.opponent.action.kind == ActionKind::Cross || self.engine.opponent.action.kind == ActionKind::Hook { 30 } else { 0 };

            // Left Glove
            fb.draw_rect(cx - 48, body_y - 35 + l_punch, 20, 20, 0xFF0F172A);
            fb.draw_rect(cx - 46, body_y - 33 + l_punch, 16, 16, 0xFFDC2626);

            // Right Glove
            fb.draw_rect(cx + 28, body_y - 35 + r_punch, 20, 20, 0xFF0F172A);
            fb.draw_rect(cx + 30, body_y - 33 + r_punch, 16, 16, 0xFFDC2626);
        }
    }

    fn render_player_boxer(&self, fb: &mut FrameBuffer, cx: i32, cy: i32, w: i32, _h: i32) {
        if self.use_3d {
            if let Some(model) = &self.hero_model {
                let mut r = self.rasterizer.borrow_mut();

                // 3D Player transform:
                // Base rotation: 128 BAM (180 deg) so player faces into the ring away from camera
                let base_rot = Mat4Fixed::from_rotation_y(128);

                // Player kinematics pose:
                let player_pose = if self.engine.player.action.kind == ActionKind::Down {
                    MechaPose3D::down()
                } else if self.engine.player.action.kind == ActionKind::HitStagger {
                    MechaPose3D::hit_stagger()
                } else if self.engine.player.action.kind == ActionKind::Guard {
                    MechaPose3D::guard()
                } else if self.engine.player.action.kind == ActionKind::SlipLeft {
                    MechaPose3D::slip(true)
                } else if self.engine.player.action.kind == ActionKind::SlipRight {
                    MechaPose3D::slip(false)
                } else if self.engine.player.action.kind == ActionKind::Jab {
                    MechaPose3D::jab(500)
                } else if self.engine.player.action.kind == ActionKind::Cross
                    || self.engine.player.action.kind == ActionKind::Finisher
                {
                    MechaPose3D::cross(500)
                } else if self.engine.player.action.kind == ActionKind::Hook {
                    MechaPose3D::hook(500)
                } else {
                    MechaPose3D::idle(self.engine.tick)
                };

                let transforms = evaluate_forward_kinematics(&player_pose, false);

                // Translation: offset by player horizontal slip
                let slip_m = (cx - w / 2) as f32 * 0.0035;
                let drop_y = if self.engine.player.action.kind == ActionKind::Down {
                    -0.55
                } else {
                    -0.38
                };
                let trans_mat =
                    Mat4Fixed::from_translation(Vec3Fixed::from_f32(slip_m, drop_y, -0.2));

                let world_mat = base_rot.mul_mat(&trans_mat);
                let cam_dist = Fixed::from_raw((1.85 * 65536.0) as i64);
                let focal_length = 520;

                r.render_model_posed(fb, model, &transforms, &world_mat, cam_dist, focal_length);
                return;
            }
        }

        let is_guard = self.engine.player.action.kind == ActionKind::Guard;
        let is_hit = self.engine.player.action.kind == ActionKind::HitStagger;
        let is_down = self.engine.player.action.kind == ActionKind::Down;

        let body_y = if is_down { cy + 50 } else { cy };
        let body_col = if is_hit { 0xFFFFFFFF } else { 0xFF1E293B };

        // Player Back & Shoulders (Over-the-shoulder rear view)
        fb.draw_rect(cx - 44, body_y - 64, 88, 70, 0xFF020617); // Ink Outline
        fb.draw_rect(cx - 40, body_y - 60, 80, 62, body_col);   // Rear Armor
        fb.draw_rect(cx - 20, body_y - 50, 40, 40, 0xFF0F172A); // Spine / Exhaust Core

        // Core Heat Glow
        let heat_ratio = (self.engine.player.heat as i32 * 10) / (self.engine.player.max_heat as i32).max(1);
        let heat_col = if heat_ratio > 7 { 0xFFEF4444 } else if heat_ratio > 3 { 0xFFF97316 } else { 0xFF06B6D4 };
        fb.draw_rect(cx - 8, body_y - 38, 16, 16, heat_col);

        // Player Gloves
        if !is_down {
            let punch_y = match self.engine.player.action.kind {
                ActionKind::Jab => -45,
                ActionKind::Cross => -55,
                ActionKind::Hook => -50,
                ActionKind::Finisher => -70,
                _ => 0,
            };

            let (glove_l_x, glove_l_y) = if is_guard { (cx - 16, body_y - 56) } else { (cx - 52, body_y - 40 + punch_y) };
            let (glove_r_x, glove_r_y) = if is_guard { (cx + 2, body_y - 56) } else { (cx + 34, body_y - 40 + punch_y) };

            // Left Glove (Player)
            fb.draw_rect(glove_l_x, glove_l_y, 24, 24, 0xFF020617);
            fb.draw_rect(glove_l_x + 2, glove_l_y + 2, 20, 20, 0xFF2563EB); // Royal Blue Gloves

            // Right Glove (Player)
            fb.draw_rect(glove_r_x, glove_r_y, 24, 24, 0xFF020617);
            fb.draw_rect(glove_r_x + 2, glove_r_y + 2, 20, 20, 0xFF2563EB);
        }
    }

    fn render_hud(&self, fb: &mut FrameBuffer, w: i32, _h: i32) {
        // Top Opponent Bar
        fb.draw_rect_blend(w / 2 - 180, 16, 360, 26, 0xDD0B0F19);
        fb.draw_rect(w / 2 - 176, 20, 352, 18, 0xFF1E293B);

        let opp_ratio = (self.engine.opponent.hp as usize * 352) / (self.engine.opponent.max_hp as usize).max(1);
        fb.draw_rect(w / 2 - 176, 20, opp_ratio as i32, 18, 0xFFDC2626);
        let opp_label = format!("OPPONENT: {} (STAGE {})", self.engine.opponent_kind.name(), self.engine.stage);
        fb.draw_text(w / 2 - 170, 25, &opp_label, 0xFFFFFFFF);

        // Feedback Banner
        if self.feedback_timer > 0 {
            let msg_len = (self.feedback_msg.len() as i32) * 4;
            let bx = (w - msg_len) / 2;
            fb.draw_rect_blend(bx - 12, 50, msg_len + 24, 18, 0xEE0B0F19);
            fb.draw_text(bx, 55, self.feedback_msg, self.feedback_color);
        }

        // Bottom Left Player HP & Heat Gauges
        fb.draw_rect_blend(20, 430, 230, 95, 0xEE0B0F19);
        fb.draw_text(30, 440, "PLAYER INTEGRITY (HP)", 0xFFE2E8F0);
        fb.draw_rect(30, 452, 200, 14, 0xFF1E293B);
        let hp_ratio = (self.engine.player.hp as usize * 200) / (self.engine.player.max_hp as usize).max(1);
        fb.draw_rect(30, 452, hp_ratio as i32, 14, 0xFF10B981);

        fb.draw_text(30, 474, "CORE HEAT (OVERHEAT RISK)", 0xFFF97316);
        fb.draw_rect(30, 486, 200, 12, 0xFF1E293B);
        let heat_ratio = (self.engine.player.heat as usize * 200) / (self.engine.player.max_heat as usize).max(1);
        let heat_col = if self.engine.player.heat > 80 { 0xFFDC2626 } else { 0xFFF97316 };
        fb.draw_rect(30, 486, heat_ratio as i32, 12, heat_col);

        let stat_str = format!("SCORE: {} | STAGE: {} | PWR: +{}", self.engine.score, self.engine.stage, self.engine.bonus_power);
        fb.draw_text(30, 508, &stat_str, 0xFFFBBF24);

        // Bottom Right Tactical Cards & Controls
        fb.draw_rect_blend(w - 280, 430, 260, 95, 0xEE0B0F19);
        let mode_tag = if self.use_3d { "3D PINEDA" } else { "2D SPRITE" };
        let tag_col = if self.use_3d { 0xFF38BDF8 } else { 0xFF94A3B8 };
        fb.draw_text(w - 270, 440, &format!("CONTROLS [F4: {}]", mode_tag), tag_col);
        fb.draw_text(w - 270, 456, "A / D : SLIP WEAVE EVADE", 0xFFE2E8F0);
        fb.draw_text(w - 270, 470, "S     : TITAN GUARD BLOCK", 0xFFE2E8F0);
        fb.draw_text(w - 270, 484, "J/K/L : JAB / CROSS / HOOK", 0xFF38BDF8);
        fb.draw_text(w - 270, 498, "SPACE : 2D ANIME CUTIN FINISHER", 0xFFF43F5E);
    }

    fn render_draft_overlay(&self, fb: &mut FrameBuffer, w: i32, h: i32) {
        fb.draw_rect_blend(0, 0, w, h, 0xDD050811);

        // Header Title Banner
        fb.draw_rect_blend(w / 2 - 240, 45, 480, 44, 0xEE0F172A);
        fb.draw_rect(w / 2 - 238, 47, 476, 40, 0xFF334155);
        fb.draw_text_2x(w / 2 - 210, 52, "★ STAGE CLEARED! ★", 0xFFFBBF24);
        fb.draw_text(w / 2 - 160, 74, "SELECT 1 OF 3 TACTICAL REPAIR UPGRADES", 0xFFE2E8F0);

        let card_w = 190;
        let card_h = 240;
        let gap = 20;
        let start_x = (w - (card_w * 3 + gap * 2)) / 2;
        let start_y = 115;

        for i in 0..3 {
            let cx = start_x + (i as i32) * (card_w + gap);
            let cy = start_y;
            let card = self.engine.draft_options[i];

            fb.draw_rect(cx, cy, card_w, card_h, 0xFF020617);
            fb.draw_rect(cx + 2, cy + 2, card_w - 4, card_h - 4, 0xFF1E293B);

            let (bar_col, text_col) = match i {
                0 => (0xFF0284C7, 0xFF38BDF8),
                1 => (0xFFD97706, 0xFFFBBF24),
                _ => (0xFF7C3AED, 0xFFC084FC),
            };
            fb.draw_rect(cx + 4, cy + 4, card_w - 8, 30, bar_col);
            let key_char = match i { 0 => 'J', 1 => 'K', _ => 'L' };
            fb.draw_text(cx + 10, cy + 12, &format!("[KEY {} / {}]", i + 1, key_char), 0xFFFFFFFF);

            fb.draw_text(cx + 10, cy + 45, card.kind.name(), text_col);

            fb.draw_rect(cx + 8, cy + 65, card_w - 16, 50, 0xFF0F172A);
            fb.draw_text(cx + 14, cy + 72, &format!("POWER: +{}", card.power), 0xFFF87171);
            fb.draw_text(cx + 14, cy + 86, &format!("HEAT:  +{}", card.heat), 0xFFFB923C);
            fb.draw_text(cx + 14, cy + 100, &format!("GUARD: +{}", card.guard), 0xFF34D399);

            fb.draw_rect(cx + 8, cy + 125, card_w - 16, 75, 0xFF0B0F19);
            fb.draw_text(cx + 12, cy + 135, card.kind.description(), 0xFFE2E8F0);

            fb.draw_rect(cx + 12, cy + 208, card_w - 24, 22, bar_col);
            fb.draw_text(cx + 35, cy + 214, &format!("EQUIP CARD [{}]", i + 1), 0xFFFFFFFF);
        }

        fb.draw_rect_blend(w / 2 - 200, h - 55, 400, 28, 0xEE0F172A);
        fb.draw_text(w / 2 - 180, h - 46, "PRESS [1, 2, 3] OR [J, K, L] TO EQUIP & ENTER NEXT STAGE", 0xFF94A3B8);
    }

    fn render_defeat_overlay(&self, fb: &mut FrameBuffer, w: i32, h: i32) {
        fb.draw_rect_blend(0, 0, w, h, 0xEE1E0505);
        fb.draw_rect_blend(w / 2 - 200, h / 2 - 50, 400, 100, 0xEE0B0F19);
        fb.draw_text_2x(w / 2 - 130, h / 2 - 35, "MECHA DOWN!", 0xFFEF4444);
        fb.draw_text(w / 2 - 100, h / 2 + 5, &format!("FINAL SCORE: {}", self.engine.score), 0xFFFBBF24);
        fb.draw_text(w / 2 - 140, h / 2 + 25, "PRESS SPACE TO REBOOT MECHA RUN", 0xFFE2E8F0);
    }
}

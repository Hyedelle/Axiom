//! Comprehensive Integration Tests for Steam Workshop UGC Publisher & Standalone .axmod Packager.

use axiom_render::modding::{
    SteamUgcConfig, UgcVisibility, WorkshopManager, WorkshopModManifest, WorkshopPublisher,
    AXMOD_MAGIC, AXMOD_VERSION,
};
use std::fs;
use std::path::Path;

#[test]
fn test_workshop_mod_validation() {
    let test_dir = Path::new("tmp/test_mod_valid");
    let _ = fs::remove_dir_all(test_dir);
    fs::create_dir_all(test_dir).unwrap();

    let manifest = WorkshopModManifest {
        mod_id: "mod_test_arm".to_string(),
        title: "Experimental Vulcan Fist".to_string(),
        author: "Hyedelle".to_string(),
        version: "1.2.0".to_string(),
        target_slot: "Arm".to_string(),
        target_mesh_name: "hero_glove_R".to_string(),
        atk_bonus: 14,
        def_bonus: 2,
        heat_bonus: 4,
        weight_bonus: 5,
        tags: vec!["Weapon".to_string(), "Heavy".to_string()],
    };

    WorkshopManager::create_template_mod(test_dir, &manifest).unwrap();
    let mod_subdir = test_dir.join("mod_test_arm");

    // Add dummy preview file
    fs::write(mod_subdir.join("preview.raw"), [0u8; 256]).unwrap();

    let report = WorkshopPublisher::validate_mod(&mod_subdir).unwrap();
    assert!(report.is_valid);
    assert_eq!(report.mod_id, "mod_test_arm");
    assert_eq!(report.target_slot, "Arm");
    assert!(report.has_manifest);
    assert!(report.has_preview);
    assert!(report.has_texture);
    assert!(report.errors.is_empty());

    let _ = fs::remove_dir_all(test_dir);
}

#[test]
fn test_steam_vdf_generation() {
    let test_dir = Path::new("tmp/test_vdf");
    let _ = fs::remove_dir_all(test_dir);
    fs::create_dir_all(test_dir).unwrap();

    let config = SteamUgcConfig {
        app_id: 3245670,
        published_file_id: 99887766,
        title: "Titan Titanium Core".to_string(),
        description: "Heavy reinforced boiler core module with heat suppression.".to_string(),
        changelog: "Updated damage mitigation ratios.".to_string(),
        visibility: UgcVisibility::Public,
        tags: vec!["Core".to_string(), "SteamBoiler".to_string()],
    };

    let vdf = WorkshopPublisher::generate_steam_vdf(test_dir, &config).unwrap();
    assert!(vdf.contains("\"appid\" \"3245670\""));
    assert!(vdf.contains("\"publishedfileid\" \"99887766\""));
    assert!(vdf.contains("\"visibility\" \"0\""));
    assert!(vdf.contains("\"title\" \"Titan Titanium Core\""));
    assert!(vdf.contains("\"changenote\" \"Updated damage mitigation ratios.\""));

    let _ = fs::remove_dir_all(test_dir);
}

#[test]
fn test_axmod_pack_unpack_roundtrip() {
    let base_src = Path::new("tmp/test_pack_src");
    let target_dir = Path::new("tmp/test_pack_dest");

    let _ = fs::remove_dir_all(base_src);
    let _ = fs::remove_dir_all(target_dir);

    let manifest = WorkshopModManifest::default();
    let source_dir = WorkshopManager::create_template_mod(base_src, &manifest).unwrap();
    let axmod_path = base_src.join("test_mod.axmod");

    // Pack into standalone .axmod
    let info = WorkshopPublisher::pack_axmod(&source_dir, &axmod_path).unwrap();
    assert_eq!(info.mod_id, manifest.mod_id);
    assert!(info.file_count >= 2);
    assert!(info.package_size > 0);
    assert!(info.checksum > 0);

    // Verify binary magic header
    let bytes = fs::read(&axmod_path).unwrap();
    assert_eq!(&bytes[0..4], &AXMOD_MAGIC);
    let version = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
    assert_eq!(version, AXMOD_VERSION);

    // Unpack into destination directory
    let extracted = WorkshopPublisher::unpack_axmod(&axmod_path, target_dir).unwrap();
    assert!(extracted >= 2);
    assert!(target_dir.join("mod.json").exists());
    assert!(target_dir.join("hazard_plate.raw").exists());

    // Clean up
    let _ = fs::remove_dir_all("tmp/test_pack_src");
    let _ = fs::remove_dir_all("tmp/test_pack_dest");
}

#[test]
fn test_publish_dry_run_workflow() {
    let base_dir = Path::new("tmp/test_dry_run");
    let _ = fs::remove_dir_all(base_dir);

    let manifest = WorkshopModManifest {
        mod_id: "mod_hydra_leg".to_string(),
        title: "Hydraulic Rapid Leg Piston".to_string(),
        author: "AxiomFoundry".to_string(),
        version: "2.0.0".to_string(),
        target_slot: "Leg".to_string(),
        target_mesh_name: "hero_leg_L".to_string(),
        atk_bonus: 0,
        def_bonus: 6,
        heat_bonus: -5,
        weight_bonus: 8,
        tags: vec!["Legs".to_string(), "Hydraulic".to_string()],
    };

    let mod_dir = WorkshopManager::create_template_mod(base_dir, &manifest).unwrap();
    let config = SteamUgcConfig::default();

    let result = WorkshopPublisher::publish_dry_run(&mod_dir, &config).unwrap();
    assert!(result.ready_for_steam);
    assert_eq!(result.mod_id, "mod_hydra_leg");
    assert!(result.vdf_path.exists());
    assert!(result.axmod_path.is_some());
    assert!(result.axmod_path.as_ref().unwrap().exists());

    let _ = fs::remove_dir_all(base_dir);
}

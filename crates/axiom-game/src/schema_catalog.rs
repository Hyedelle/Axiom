//! Factory RPG Example Schema Catalog & Adapter (`crates/axiom-game/src/schema_catalog.rs`)
//!
//! Demonstrates how domain-specific game rules, entities, and recipes
//! (Factory RPG: drills, smelters, turrets, belts, chests, ores, ingots)
//! are cleanly registered into the data-driven Axiom Engine SDK (`axiom-core::schema`).

use axiom_core::schema::{category, RecipeDefinition, RecipeRegistry, TypeDefinition, TypeRegistry};

/// Domain Type Identifiers for the Factory RPG Example
pub mod factory_types {
    pub const TYPE_EMPTY: u16 = 0;

    // --- Structures (10 ~ 99) ---
    pub const TYPE_DRILL: u16 = 10;
    pub const TYPE_BELT: u16 = 11;
    pub const TYPE_INSERTER: u16 = 12;
    pub const TYPE_SMELTER: u16 = 13;
    pub const TYPE_CHEST: u16 = 14;
    pub const TYPE_WALL: u16 = 15;
    pub const TYPE_TURRET: u16 = 16;
    pub const TYPE_BASE_HEART: u16 = 17;

    // --- Raw Resources (100 ~ 199) ---
    pub const ITEM_IRON_ORE: u16 = 101;
    pub const ITEM_COPPER_ORE: u16 = 102;
    pub const ITEM_STONE: u16 = 103;
    pub const ITEM_COAL: u16 = 104;

    // --- Processed Products (200 ~ 299) ---
    pub const ITEM_IRON_PLATE: u16 = 201;
    pub const ITEM_COPPER_PLATE: u16 = 202;
    pub const ITEM_STEEL: u16 = 203;
}

/// Populates an engine `TypeRegistry` with all Factory RPG types and attributes.
pub fn register_factory_types<const CAP: usize>(registry: &mut TypeRegistry<CAP>) {
    use factory_types::*;

    // 1. Mining Drill
    let mut drill = TypeDefinition::empty();
    drill.id = TYPE_DRILL;
    drill.category_flags = category::SOLID | category::PRODUCER;
    drill.default_hp = 300;
    drill.base_speed = 100; // 1.0x
    drill.primary_color = 0x5080D0; // Steel blue
    registry.register(drill);

    // 2. Logistics Belt
    let mut belt = TypeDefinition::empty();
    belt.id = TYPE_BELT;
    belt.category_flags = category::PASSABLE | category::TRANSPORTER;
    belt.default_hp = 100;
    belt.base_speed = 200; // 2.0x
    belt.primary_color = 0x409040; // Green
    registry.register(belt);

    // 3. Inserter Arm
    let mut inserter = TypeDefinition::empty();
    inserter.id = TYPE_INSERTER;
    inserter.category_flags = category::SOLID | category::TRANSPORTER;
    inserter.default_hp = 150;
    inserter.base_speed = 100;
    inserter.primary_color = 0xD0A020; // Yellow-amber
    registry.register(inserter);

    // 4. Smelter Furnace
    let mut smelter = TypeDefinition::empty();
    smelter.id = TYPE_SMELTER;
    smelter.category_flags = category::SOLID | category::CONSUMER | category::PRODUCER;
    smelter.default_hp = 500;
    smelter.base_speed = 100;
    smelter.primary_color = 0xD04020; // Orange-red
    registry.register(smelter);

    // 5. Storage Chest
    let mut chest = TypeDefinition::empty();
    chest.id = TYPE_CHEST;
    chest.category_flags = category::SOLID | category::CONTAINER;
    chest.default_hp = 200;
    chest.primary_color = 0x8B5A2B; // Brown
    registry.register(chest);

    // 6. Defensive Wall
    let mut wall = TypeDefinition::empty();
    wall.id = TYPE_WALL;
    wall.category_flags = category::SOLID | category::DEFENSIVE;
    wall.default_hp = 500;
    wall.primary_color = 0x707070; // Dark grey
    registry.register(wall);

    // 7. Automated Turret
    let mut turret = TypeDefinition::empty();
    turret.id = TYPE_TURRET;
    turret.category_flags = category::SOLID | category::DEFENSIVE | category::COMBATANT;
    turret.default_hp = 250;
    turret.base_power = 25; // Base damage
    turret.primary_color = 0x20B2AA; // Light sea green
    registry.register(turret);

    // 8. Base Heart
    let mut heart = TypeDefinition::empty();
    heart.id = TYPE_BASE_HEART;
    heart.category_flags = category::SOLID | category::CONTAINER | category::DEFENSIVE;
    heart.default_hp = 1000;
    heart.primary_color = 0xFF2050; // Crimson
    registry.register(heart);

    // --- Items ---
    let mut iron_ore = TypeDefinition::empty();
    iron_ore.id = ITEM_IRON_ORE;
    iron_ore.category_flags = category::RAW_MATERIAL;
    iron_ore.primary_color = 0x708090;
    registry.register(iron_ore);

    let mut copper_ore = TypeDefinition::empty();
    copper_ore.id = ITEM_COPPER_ORE;
    copper_ore.category_flags = category::RAW_MATERIAL;
    copper_ore.primary_color = 0xB87333;
    registry.register(copper_ore);

    let mut iron_plate = TypeDefinition::empty();
    iron_plate.id = ITEM_IRON_PLATE;
    iron_plate.category_flags = category::MANUFACTURED;
    iron_plate.primary_color = 0xC0C0D0;
    registry.register(iron_plate);

    let mut copper_plate = TypeDefinition::empty();
    copper_plate.id = ITEM_COPPER_PLATE;
    copper_plate.category_flags = category::MANUFACTURED;
    copper_plate.primary_color = 0xD48A4A;
    registry.register(copper_plate);

    let mut steel = TypeDefinition::empty();
    steel.id = ITEM_STEEL;
    steel.category_flags = category::MANUFACTURED;
    steel.primary_color = 0x4A5A6A;
    registry.register(steel);
}

/// Populates an engine `RecipeRegistry` with all Factory RPG transformation recipes.
pub fn register_factory_recipes<const CAP: usize>(registry: &mut RecipeRegistry<CAP>) {
    use factory_types::*;

    // Recipe 1: Smelt Iron (2 Iron Ore -> 1 Iron Plate in 3 ticks)
    let mut r1 = RecipeDefinition::empty();
    r1.id = 1;
    r1.input_id = ITEM_IRON_ORE;
    r1.input_count = 2;
    r1.output_id = ITEM_IRON_PLATE;
    r1.output_count = 1;
    r1.required_ticks = 3;
    r1.required_category = category::CONSUMER;
    registry.register(r1);

    // Recipe 2: Smelt Copper (2 Copper Ore -> 1 Copper Plate in 3 ticks)
    let mut r2 = RecipeDefinition::empty();
    r2.id = 2;
    r2.input_id = ITEM_COPPER_ORE;
    r2.input_count = 2;
    r2.output_id = ITEM_COPPER_PLATE;
    r2.output_count = 1;
    r2.required_ticks = 3;
    r2.required_category = category::CONSUMER;
    registry.register(r2);

    // Recipe 3: Produce Steel (3 Iron Plates + 1 Coal -> 1 Steel Ingot in 6 ticks)
    let mut r3 = RecipeDefinition::empty();
    r3.id = 3;
    r3.input_id = ITEM_IRON_PLATE;
    r3.input_count = 3;
    r3.secondary_input_id = ITEM_COAL;
    r3.secondary_input_count = 1;
    r3.output_id = ITEM_STEEL;
    r3.output_count = 1;
    r3.required_ticks = 6;
    r3.required_category = category::CONSUMER;
    registry.register(r3);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_factory_schema_registration() {
        let mut types = TypeRegistry::<64>::new();
        let mut recipes = RecipeRegistry::<32>::new();

        register_factory_types(&mut types);
        register_factory_recipes(&mut recipes);

        // Verify types
        let drill = types.get(factory_types::TYPE_DRILL).expect("Drill should be registered");
        assert_eq!(drill.default_hp, 300);
        assert_eq!(drill.primary_color, 0x5080D0);
        assert!(drill.has_category(category::PRODUCER));

        let smelter = types.get(factory_types::TYPE_SMELTER).expect("Smelter should be registered");
        assert_eq!(smelter.default_hp, 500);
        assert!(smelter.has_category(category::CONSUMER));

        // Verify recipes
        let iron_smelt = recipes.find_by_input(factory_types::ITEM_IRON_ORE).expect("Iron smelting recipe exists");
        assert_eq!(iron_smelt.input_count, 2);
        assert_eq!(iron_smelt.output_id, factory_types::ITEM_IRON_PLATE);
        assert_eq!(iron_smelt.output_count, 1);
        assert_eq!(iron_smelt.required_ticks, 3);
    }
}

//! # Data-Driven Schema & Zero-Allocation Registry (T-046)
//!
//! Provides hardware cache-line aligned (64-byte), zero-float, zero-heap data
//! definitions for generic entities, items, machines, and transformation recipes.
//!
//! This module decouples the Axiom engine core from hardcoded game-specific enums
//! (such as drills, smelters, turrets), allowing any game or mod to define its
//! simulation primitives via flat binary data tables.

use crate::hash::deterministic_hash;

/// Magic header for serialized Axiom schema data files ("AXIOMSCH" in ASCII).
pub const SCHEMA_MAGIC: u64 = 0x4158494F4D534348;

/// Default maximum number of registered type definitions in a static registry slab.
pub const DEFAULT_TYPE_REGISTRY_CAPACITY: usize = 256;

/// Default maximum number of registered transformation recipes.
pub const DEFAULT_RECIPE_REGISTRY_CAPACITY: usize = 128;

// ---------------------------------------------------------------------------
// Category Bitflags
// ---------------------------------------------------------------------------

pub mod category {
    pub const NONE: u16 = 0;
    pub const SOLID: u16 = 1 << 0;
    pub const PASSABLE: u16 = 1 << 1;
    pub const INTERACTABLE: u16 = 1 << 2;
    pub const PRODUCER: u16 = 1 << 3;
    pub const CONSUMER: u16 = 1 << 4;
    pub const TRANSPORTER: u16 = 1 << 5;
    pub const CONTAINER: u16 = 1 << 6;
    pub const DEFENSIVE: u16 = 1 << 7;
    pub const COMBATANT: u16 = 1 << 8;
    pub const RESOURCE_NODE: u16 = 1 << 9;
    pub const RAW_MATERIAL: u16 = 1 << 10;
    pub const MANUFACTURED: u16 = 1 << 11;
}

// ---------------------------------------------------------------------------
// 64-byte Cache-Line Aligned Type Definition
// ---------------------------------------------------------------------------

/// 64-byte cache-line aligned definition record for any entity, item, or tile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(C, align(64))]
pub struct TypeDefinition {
    /// Unique identifier for this type (0 = unassigned / empty).
    pub id: u16,
    /// Bitflags from [`category`] describing the behavior of this type.
    pub category_flags: u16,
    /// Base maximum hit points (0 for non-destructible items).
    pub default_hp: u16,
    /// Base movement speed or production rate (integer ticks or speed units).
    pub base_speed: u16,
    /// Base attack or processing power.
    pub base_power: u16,
    /// Interaction, attack, or transfer range in tiles.
    pub range: u16,
    /// Action cooldown in integer ticks.
    pub cooldown: u16,
    /// Storage capacity or item stack limit.
    pub capacity: u16,
    /// Primary ARGB 32-bit color for software rasterization.
    pub primary_color: u32,
    /// Secondary ARGB 32-bit color for accents/details.
    pub secondary_color: u32,
    /// Custom game-specific numeric tag (e.g. tier, faction, element).
    pub custom_tag: u32,
    /// Reserved padding to ensure exactly 64 bytes.
    pub _reserved: [u8; 36],
}

impl Default for TypeDefinition {
    #[inline(always)]
    fn default() -> Self {
        Self::empty()
    }
}

impl TypeDefinition {
    pub const fn empty() -> Self {
        Self {
            id: 0,
            category_flags: category::NONE,
            default_hp: 0,
            base_speed: 0,
            base_power: 0,
            range: 0,
            cooldown: 0,
            capacity: 0,
            primary_color: 0,
            secondary_color: 0,
            custom_tag: 0,
            _reserved: [0u8; 36],
        }
    }

    #[inline(always)]
    pub const fn is_valid(&self) -> bool {
        self.id != 0
    }

    #[inline(always)]
    pub const fn has_category(&self, flag: u16) -> bool {
        (self.category_flags & flag) == flag
    }
}

// ---------------------------------------------------------------------------
// 64-byte Cache-Line Aligned Recipe Definition
// ---------------------------------------------------------------------------

/// 64-byte cache-line aligned recipe record for item transformation / crafting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(C, align(64))]
pub struct RecipeDefinition {
    /// Unique recipe identifier (0 = empty).
    pub id: u16,
    /// Primary input type ID.
    pub input_id: u16,
    /// Primary input item count required.
    pub input_count: u16,
    /// Secondary input type ID (0 if none).
    pub secondary_input_id: u16,
    /// Secondary input item count required.
    pub secondary_input_count: u16,
    /// Output type ID produced upon completion.
    pub output_id: u16,
    /// Output item count produced.
    pub output_count: u16,
    /// Required processing time in integer ticks.
    pub required_ticks: u16,
    /// Required machine category flag (e.g. `PRODUCER` or `CONSUMER`).
    pub required_category: u16,
    /// Reserved padding to ensure exactly 64 bytes.
    pub _reserved: [u8; 46],
}

impl Default for RecipeDefinition {
    #[inline(always)]
    fn default() -> Self {
        Self::empty()
    }
}

impl RecipeDefinition {
    pub const fn empty() -> Self {
        Self {
            id: 0,
            input_id: 0,
            input_count: 0,
            secondary_input_id: 0,
            secondary_input_count: 0,
            output_id: 0,
            output_count: 0,
            required_ticks: 0,
            required_category: category::NONE,
            _reserved: [0u8; 46],
        }
    }

    #[inline(always)]
    pub const fn is_valid(&self) -> bool {
        self.id != 0
    }
}

// ---------------------------------------------------------------------------
// Static Type Registry
// ---------------------------------------------------------------------------

/// Zero-heap flat slab registry for [`TypeDefinition`] lookup and iteration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeRegistry<const CAP: usize = DEFAULT_TYPE_REGISTRY_CAPACITY> {
    pub types: [TypeDefinition; CAP],
    pub count: usize,
}

impl<const CAP: usize> Default for TypeRegistry<CAP> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const CAP: usize> TypeRegistry<CAP> {
    pub const fn new() -> Self {
        Self {
            types: [TypeDefinition::empty(); CAP],
            count: 0,
        }
    }

    /// Registers a new type definition into the flat registry.
    ///
    /// Returns `true` if registration succeeded, or `false` if the registry is full
    /// or `def.id == 0`. Overwrites existing entry if the same ID is re-registered.
    pub fn register(&mut self, def: TypeDefinition) -> bool {
        if def.id == 0 {
            return false;
        }

        // Check if updating existing
        for i in 0..self.count {
            if self.types[i].id == def.id {
                self.types[i] = def;
                return true;
            }
        }

        if self.count >= CAP {
            return false;
        }

        self.types[self.count] = def;
        self.count += 1;
        true
    }

    /// O(N) lookup by type ID (constant time across hardware prefetch flat array).
    #[inline(always)]
    pub fn get(&self, id: u16) -> Option<&TypeDefinition> {
        if id == 0 {
            return None;
        }
        for i in 0..self.count {
            if self.types[i].id == id {
                return Some(&self.types[i]);
            }
        }
        None
    }

    /// Returns the number of registered types.
    #[inline(always)]
    pub const fn len(&self) -> usize {
        self.count
    }

    /// Returns whether the registry is empty.
    #[inline(always)]
    pub const fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Computes deterministic 64-bit FNV hash of the entire registry state.
    pub fn compute_hash(&self) -> u64 {
        let mut h = 0xCBF2_9CE4_8422_2325u64;
        for i in 0..self.count {
            let slice = unsafe {
                core::slice::from_raw_parts(
                    &self.types[i] as *const TypeDefinition as *const u8,
                    core::mem::size_of::<TypeDefinition>(),
                )
            };
            h ^= deterministic_hash(slice);
            h = h.wrapping_mul(0x0000_0100_0000_01B3);
        }
        h
    }
}

// ---------------------------------------------------------------------------
// Static Recipe Registry
// ---------------------------------------------------------------------------

/// Zero-heap flat slab registry for [`RecipeDefinition`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecipeRegistry<const CAP: usize = DEFAULT_RECIPE_REGISTRY_CAPACITY> {
    pub recipes: [RecipeDefinition; CAP],
    pub count: usize,
}

impl<const CAP: usize> Default for RecipeRegistry<CAP> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const CAP: usize> RecipeRegistry<CAP> {
    pub const fn new() -> Self {
        Self {
            recipes: [RecipeDefinition::empty(); CAP],
            count: 0,
        }
    }

    /// Registers a new recipe definition.
    pub fn register(&mut self, recipe: RecipeDefinition) -> bool {
        if recipe.id == 0 || self.count >= CAP {
            return false;
        }

        for i in 0..self.count {
            if self.recipes[i].id == recipe.id {
                self.recipes[i] = recipe;
                return true;
            }
        }

        self.recipes[self.count] = recipe;
        self.count += 1;
        true
    }

    /// Finds a recipe by its unique ID.
    #[inline(always)]
    pub fn find_by_id(&self, id: u16) -> Option<&RecipeDefinition> {
        if id == 0 {
            return None;
        }
        for i in 0..self.count {
            if self.recipes[i].id == id {
                return Some(&self.recipes[i]);
            }
        }
        None
    }

    /// Finds the first matching recipe that consumes `input_id`.
    #[inline(always)]
    pub fn find_by_input(&self, input_id: u16) -> Option<&RecipeDefinition> {
        if input_id == 0 {
            return None;
        }
        for i in 0..self.count {
            if self.recipes[i].input_id == input_id {
                return Some(&self.recipes[i]);
            }
        }
        None
    }
}

// ---------------------------------------------------------------------------
// Binary Schema Header & Serialization
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C, align(64))]
pub struct SchemaHeader {
    pub magic: u64,
    pub version: u32,
    pub type_count: u32,
    pub recipe_count: u32,
    pub _pad: u32,
    pub checksum: u64,
    pub _reserved: [u8; 32],
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_schema_structs_are_cache_line_aligned() {
        assert_eq!(core::mem::size_of::<TypeDefinition>(), 64);
        assert_eq!(core::mem::align_of::<TypeDefinition>(), 64);

        assert_eq!(core::mem::size_of::<RecipeDefinition>(), 64);
        assert_eq!(core::mem::align_of::<RecipeDefinition>(), 64);

        assert_eq!(core::mem::size_of::<SchemaHeader>(), 64);
        assert_eq!(core::mem::align_of::<SchemaHeader>(), 64);
    }

    #[test]
    fn test_type_registry_crud_and_lookup() {
        let mut reg = TypeRegistry::<16>::new();
        assert_eq!(reg.len(), 0);

        let iron_ore = TypeDefinition {
            id: 1,
            category_flags: category::RAW_MATERIAL | category::PASSABLE,
            primary_color: 0xFF94A3B8,
            capacity: 100,
            ..TypeDefinition::empty()
        };

        assert!(reg.register(iron_ore));
        assert_eq!(reg.len(), 1);

        let retrieved = reg.get(1).expect("item 1 must exist");
        assert_eq!(retrieved.id, 1);
        assert!(retrieved.has_category(category::RAW_MATERIAL));
        assert!(retrieved.has_category(category::PASSABLE));
        assert_eq!(retrieved.primary_color, 0xFF94A3B8);

        // Update existing item
        let mut iron_ore_updated = iron_ore;
        iron_ore_updated.capacity = 200;
        assert!(reg.register(iron_ore_updated));
        assert_eq!(reg.len(), 1); // count not incremented
        assert_eq!(reg.get(1).unwrap().capacity, 200);

        // Non-existent item
        assert!(reg.get(999).is_none());
    }

    #[test]
    fn test_recipe_registry_and_transformation_matching() {
        let mut reg = RecipeRegistry::<8>::new();

        let smelt_iron = RecipeDefinition {
            id: 101,
            input_id: 1, // Iron Ore
            input_count: 1,
            secondary_input_id: 0,
            secondary_input_count: 0,
            output_id: 10, // Iron Plate
            output_count: 1,
            required_ticks: 25,
            required_category: category::CONSUMER | category::PRODUCER,
            ..RecipeDefinition::empty()
        };

        assert!(reg.register(smelt_iron));

        let found = reg.find_by_input(1).expect("recipe for iron ore must exist");
        assert_eq!(found.id, 101);
        assert_eq!(found.output_id, 10);
        assert_eq!(found.required_ticks, 25);
    }

    #[test]
    fn test_registry_hash_is_100_percent_deterministic() {
        let mut reg1 = TypeRegistry::<8>::new();
        let mut reg2 = TypeRegistry::<8>::new();

        let t1 = TypeDefinition {
            id: 42,
            category_flags: category::DEFENSIVE | category::SOLID,
            default_hp: 500,
            primary_color: 0xFF14532D,
            ..TypeDefinition::empty()
        };

        reg1.register(t1);
        reg2.register(t1);

        assert_eq!(reg1.compute_hash(), reg2.compute_hash());
        assert_ne!(reg1.compute_hash(), 0);
    }
}

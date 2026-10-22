//! Axiom SDK — Universal Data-Driven C-ABI & WASM Interface (T-048)
//!
//! Exposes a zero-allocation, cache-friendly C-ABI for orchestrating the Axiom Engine
//! from any host language (TypeScript/WASM, C#, C++, Python) without domain coupling.

use axiom_core::grid_netcode::GridRollbackSession;
use axiom_core::schema::{RecipeDefinition, RecipeRegistry, TypeDefinition, TypeRegistry};
pub use axiom_core::tile_entity::grid_action_kind;
use axiom_core::tile_entity::{GridAction, TileCell, TileGrid};

pub const DEFAULT_GRID_WIDTH: usize = 64;
pub const DEFAULT_GRID_HEIGHT: usize = 64;
pub const DEFAULT_TYPE_CAP: usize = 256;
pub const DEFAULT_RECIPE_CAP: usize = 128;
pub const DEFAULT_ROLLBACK_FRAMES: usize = 32;
pub const DEFAULT_NET_PLAYERS: usize = 2;

/// Specialized Rollback Session Type for SDK C-ABI
pub type SdkGridRollbackSession = GridRollbackSession<
    DEFAULT_GRID_WIDTH,
    DEFAULT_GRID_HEIGHT,
    DEFAULT_ROLLBACK_FRAMES,
    DEFAULT_NET_PLAYERS,
    DEFAULT_RECIPE_CAP,
>;

/// Return codes for C-ABI functions
pub mod return_code {
    pub const SUCCESS: i32 = 0;
    pub const ERR_NULL_PTR: i32 = -1;
    pub const ERR_OUT_OF_BOUNDS: i32 = -2;
    pub const ERR_REGISTRY_FULL: i32 = -3;
    pub const ERR_INVALID_ARGUMENT: i32 = -4;
    pub const ERR_OPERATION_FAILED: i32 = -5;
}

/// Universal Engine Instance Context
#[repr(C)]
pub struct EngineContext {
    pub types: TypeRegistry<DEFAULT_TYPE_CAP>,
    pub recipes: RecipeRegistry<DEFAULT_RECIPE_CAP>,
    pub grid: TileGrid<DEFAULT_GRID_WIDTH, DEFAULT_GRID_HEIGHT>,
    pub tick_count: u64,
}

impl EngineContext {
    pub fn new() -> Self {
        Self {
            types: TypeRegistry::new(),
            recipes: RecipeRegistry::new(),
            grid: TileGrid::new(),
            tick_count: 0,
        }
    }

    /// Step the engine by 1 tick: runs data-driven recipes across grid and advances tick count.
    pub fn tick(&mut self) -> u64 {
        self.grid.process_recipes(&self.recipes);
        self.tick_count = self.tick_count.wrapping_add(1);
        self.tick_count
    }

    /// Returns the combined deterministic 64-bit state hash.
    pub fn compute_state_hash(&self) -> u64 {
        const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
        const FNV_PRIME: u64 = 0x100000001b3;

        let mut hash = FNV_OFFSET_BASIS;
        hash ^= self.types.compute_hash();
        hash = hash.wrapping_mul(FNV_PRIME);
        hash ^= self.grid.state_hash();
        hash = hash.wrapping_mul(FNV_PRIME);
        hash ^= self.tick_count;
        hash = hash.wrapping_mul(FNV_PRIME);
        hash
    }
}

impl Default for EngineContext {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// C-ABI: Engine Lifecycle
// ---------------------------------------------------------------------------

/// Allocates and initializes a new Axiom Engine instance.
#[no_mangle]
pub extern "C" fn axiom_engine_create() -> *mut EngineContext {
    let ctx = Box::new(EngineContext::new());
    Box::into_raw(ctx)
}

/// Frees an existing Axiom Engine instance.
#[no_mangle]
pub extern "C" fn axiom_engine_destroy(ctx: *mut EngineContext) {
    if !ctx.is_null() {
        unsafe {
            drop(Box::from_raw(ctx));
        }
    }
}

/// Advances the engine simulation by 1 deterministic tick.
#[no_mangle]
pub extern "C" fn axiom_engine_tick(ctx: *mut EngineContext) -> u64 {
    if ctx.is_null() {
        return 0;
    }
    unsafe { (*ctx).tick() }
}

/// Returns the current simulation tick count.
#[no_mangle]
pub extern "C" fn axiom_engine_tick_count(ctx: *const EngineContext) -> u64 {
    if ctx.is_null() {
        return 0;
    }
    unsafe { (*ctx).tick_count }
}

/// Computes the 64-bit deterministic hash of the current engine state.
#[no_mangle]
pub extern "C" fn axiom_engine_state_hash(ctx: *const EngineContext) -> u64 {
    if ctx.is_null() {
        return 0;
    }
    unsafe { (*ctx).compute_state_hash() }
}

// ---------------------------------------------------------------------------
// C-ABI: Data-Driven Schema Registration
// ---------------------------------------------------------------------------

/// Registers a type definition (building, item, unit, etc.) into the engine schema registry.
#[no_mangle]
pub extern "C" fn axiom_schema_register_type(
    ctx: *mut EngineContext,
    type_def: *const TypeDefinition,
) -> i32 {
    if ctx.is_null() || type_def.is_null() {
        return return_code::ERR_NULL_PTR;
    }

    let def = unsafe { *type_def };
    let success = unsafe { (*ctx).types.register(def) };
    if success {
        return_code::SUCCESS
    } else {
        return_code::ERR_REGISTRY_FULL
    }
}

/// Registers a recipe definition into the engine recipe registry.
#[no_mangle]
pub extern "C" fn axiom_schema_register_recipe(
    ctx: *mut EngineContext,
    recipe_def: *const RecipeDefinition,
) -> i32 {
    if ctx.is_null() || recipe_def.is_null() {
        return return_code::ERR_NULL_PTR;
    }

    let recipe = unsafe { *recipe_def };
    let success = unsafe { (*ctx).recipes.register(recipe) };
    if success {
        return_code::SUCCESS
    } else {
        return_code::ERR_REGISTRY_FULL
    }
}

/// Looks up a registered type definition by ID and copies it into `out_def`.
#[no_mangle]
pub extern "C" fn axiom_schema_get_type(
    ctx: *const EngineContext,
    type_id: u16,
    out_def: *mut TypeDefinition,
) -> i32 {
    if ctx.is_null() || out_def.is_null() {
        return return_code::ERR_NULL_PTR;
    }

    let found = unsafe { (*ctx).types.get(type_id) };
    if let Some(def) = found {
        unsafe {
            *out_def = *def;
        }
        return_code::SUCCESS
    } else {
        return_code::ERR_INVALID_ARGUMENT
    }
}

// ---------------------------------------------------------------------------
// C-ABI: Spatial Grid & Tile Entity Manipulation
// ---------------------------------------------------------------------------

/// Places or updates a 64-byte `TileCell` at coordinates (x, y).
#[no_mangle]
pub extern "C" fn axiom_grid_set_tile(
    ctx: *mut EngineContext,
    x: u32,
    y: u32,
    cell: *const TileCell,
) -> i32 {
    if ctx.is_null() || cell.is_null() {
        return return_code::ERR_NULL_PTR;
    }

    let c = unsafe { *cell };
    let ok = unsafe { (*ctx).grid.set(x as usize, y as usize, c) };
    if ok {
        return_code::SUCCESS
    } else {
        return_code::ERR_OUT_OF_BOUNDS
    }
}

/// Retrieves a copy of the 64-byte `TileCell` at coordinates (x, y).
#[no_mangle]
pub extern "C" fn axiom_grid_get_tile(
    ctx: *const EngineContext,
    x: u32,
    y: u32,
    out_cell: *mut TileCell,
) -> i32 {
    if ctx.is_null() || out_cell.is_null() {
        return return_code::ERR_NULL_PTR;
    }

    let cell_opt = unsafe { (*ctx).grid.get(x as usize, y as usize) };
    if let Some(c) = cell_opt {
        unsafe {
            *out_cell = *c;
        }
        return_code::SUCCESS
    } else {
        return_code::ERR_OUT_OF_BOUNDS
    }
}

/// Pushes items into the inventory of the tile at (x, y).
#[no_mangle]
pub extern "C" fn axiom_grid_push_item(
    ctx: *mut EngineContext,
    x: u32,
    y: u32,
    item_type: u16,
    count: u16,
) -> i32 {
    if ctx.is_null() {
        return return_code::ERR_NULL_PTR;
    }

    let cell_opt = unsafe { (*ctx).grid.get_mut(x as usize, y as usize) };
    if let Some(c) = cell_opt {
        if c.push_item(item_type, count) {
            return_code::SUCCESS
        } else {
            return_code::ERR_OPERATION_FAILED
        }
    } else {
        return_code::ERR_OUT_OF_BOUNDS
    }
}

/// Pops items from the inventory of the tile at (x, y).
#[no_mangle]
pub extern "C" fn axiom_grid_pop_item(
    ctx: *mut EngineContext,
    x: u32,
    y: u32,
    item_type: u16,
    count: u16,
) -> i32 {
    if ctx.is_null() {
        return return_code::ERR_NULL_PTR;
    }

    let cell_opt = unsafe { (*ctx).grid.get_mut(x as usize, y as usize) };
    if let Some(c) = cell_opt {
        if c.pop_item(item_type, count) {
            return_code::SUCCESS
        } else {
            return_code::ERR_OPERATION_FAILED
        }
    } else {
        return_code::ERR_OUT_OF_BOUNDS
    }
}

/// Returns a raw pointer to the contiguous 64-byte `TileCell` array for zero-copy memory access.
#[no_mangle]
pub extern "C" fn axiom_grid_buffer_ptr(ctx: *mut EngineContext) -> *mut TileCell {
    if ctx.is_null() {
        core::ptr::null_mut()
    } else {
        unsafe { (*ctx).grid.cells.as_mut_ptr() as *mut TileCell }
    }
}

/// Returns the total number of cells in the grid buffer (W * H).
#[no_mangle]
pub extern "C" fn axiom_grid_buffer_len(_ctx: *const EngineContext) -> usize {
    DEFAULT_GRID_WIDTH * DEFAULT_GRID_HEIGHT
}

/// Applies a 16-byte discrete `GridAction` to the grid in `EngineContext`.
#[no_mangle]
pub extern "C" fn axiom_grid_apply_action(
    ctx: *mut EngineContext,
    action: *const GridAction,
) -> i32 {
    if ctx.is_null() || action.is_null() {
        return return_code::ERR_NULL_PTR;
    }

    let act = unsafe { *action };
    let ok = unsafe { (*ctx).grid.apply_action(&act) };
    if ok {
        return_code::SUCCESS
    } else {
        return_code::ERR_OPERATION_FAILED
    }
}

// ---------------------------------------------------------------------------
// C-ABI: Multiplayer P2P Grid Rollback Netcode (T-051)
// ---------------------------------------------------------------------------

/// Allocates and initializes a new P2P Grid Rollback Netcode Session.
#[no_mangle]
pub extern "C" fn axiom_grid_net_create(local_player_id: u32) -> *mut SdkGridRollbackSession {
    let session = GridRollbackSession::new_boxed(
        local_player_id as usize,
        TileGrid::new(),
        RecipeRegistry::new(),
    );
    Box::into_raw(session)
}

/// Destroys and frees an existing P2P Grid Rollback Netcode Session.
#[no_mangle]
pub extern "C" fn axiom_grid_net_destroy(session: *mut SdkGridRollbackSession) {
    if !session.is_null() {
        unsafe {
            drop(Box::from_raw(session));
        }
    }
}

/// Copies registered recipes from an `EngineContext` into the rollback session.
#[no_mangle]
pub extern "C" fn axiom_grid_net_copy_recipes_from_context(
    session: *mut SdkGridRollbackSession,
    ctx: *const EngineContext,
) -> i32 {
    if session.is_null() || ctx.is_null() {
        return return_code::ERR_NULL_PTR;
    }

    unsafe {
        (*session).recipes = (*ctx).recipes.clone();
    }
    return_code::SUCCESS
}

/// Advances 1 local frame with zero input latency. If `action` is null, `GridAction::NONE` is applied.
#[no_mangle]
pub extern "C" fn axiom_grid_net_advance_frame(
    session: *mut SdkGridRollbackSession,
    action: *const GridAction,
) -> u64 {
    if session.is_null() {
        return 0;
    }

    let act = if action.is_null() {
        GridAction::NONE
    } else {
        unsafe { *action }
    };

    let stats = unsafe { (*session).advance_frame(act) };
    stats.current_frame
}

/// Delivers a confirmed remote grid action arriving from network link.
/// Returns true if a speculative rollback was triggered.
#[no_mangle]
pub extern "C" fn axiom_grid_net_receive_remote(
    session: *mut SdkGridRollbackSession,
    remote_player_id: u32,
    frame: u64,
    action: *const GridAction,
) -> bool {
    if session.is_null() || action.is_null() {
        return false;
    }

    let act = unsafe { *action };
    unsafe { (*session).receive_remote_action(remote_player_id as usize, frame, act) }
}

/// Returns a raw pointer to the live `TileCell` buffer of the rollback session for zero-copy rendering.
#[no_mangle]
pub extern "C" fn axiom_grid_net_get_grid_ptr(session: *mut SdkGridRollbackSession) -> *mut TileCell {
    if session.is_null() {
        core::ptr::null_mut()
    } else {
        unsafe { (*session).current_grid.cells.as_mut_ptr() as *mut TileCell }
    }
}

/// Synchronizes the current rollback grid state back into an `EngineContext`.
#[no_mangle]
pub extern "C" fn axiom_grid_net_sync_to_context(
    session: *const SdkGridRollbackSession,
    ctx: *mut EngineContext,
) -> i32 {
    if session.is_null() || ctx.is_null() {
        return return_code::ERR_NULL_PTR;
    }

    unsafe {
        (*ctx).grid = (*session).current_grid;
        (*ctx).tick_count = (*session).current_frame;
    }
    return_code::SUCCESS
}

/// Returns the 64-bit deterministic hash of the rollback session's current grid.
#[no_mangle]
pub extern "C" fn axiom_grid_net_state_hash(session: *const SdkGridRollbackSession) -> u64 {
    if session.is_null() {
        0
    } else {
        unsafe { (*session).state_hash() }
    }
}

/// Returns total number of rollbacks executed by the session.
#[no_mangle]
pub extern "C" fn axiom_grid_net_rollbacks_count(session: *const SdkGridRollbackSession) -> u64 {
    if session.is_null() {
        0
    } else {
        unsafe { (*session).total_rollbacks }
    }
}

/// Returns total number of resimulated frames executed during rollbacks.
#[no_mangle]
pub extern "C" fn axiom_grid_net_resimulated_count(session: *const SdkGridRollbackSession) -> u64 {
    if session.is_null() {
        0
    } else {
        unsafe { (*session).total_frames_resimulated }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_core::tile_entity::tile_flags;

    #[test]
    fn test_engine_lifecycle_c_abi() {
        let engine = axiom_engine_create();
        assert!(!engine.is_null());

        assert_eq!(axiom_engine_tick_count(engine), 0);
        let tick1 = axiom_engine_tick(engine);
        assert_eq!(tick1, 1);
        assert_eq!(axiom_engine_tick_count(engine), 1);

        let hash = axiom_engine_state_hash(engine);
        assert_ne!(hash, 0);

        axiom_engine_destroy(engine);
    }

    #[test]
    fn test_schema_registration_c_abi() {
        let engine = axiom_engine_create();

        // Register Type
        let mut type_def = TypeDefinition::empty();
        type_def.id = 42;
        type_def.default_hp = 300;
        let res = axiom_schema_register_type(engine, &type_def);
        assert_eq!(res, return_code::SUCCESS);

        // Retrieve Type
        let mut out_type = TypeDefinition::empty();
        let res_get = axiom_schema_get_type(engine, 42, &mut out_type);
        assert_eq!(res_get, return_code::SUCCESS);
        assert_eq!(out_type.id, 42);
        assert_eq!(out_type.default_hp, 300);

        // Register Recipe
        let mut recipe = RecipeDefinition::empty();
        recipe.id = 1;
        recipe.input_id = 10;
        recipe.input_count = 2;
        recipe.output_id = 20;
        recipe.output_count = 1;
        recipe.required_ticks = 4;
        let res_recipe = axiom_schema_register_recipe(engine, &recipe);
        assert_eq!(res_recipe, return_code::SUCCESS);

        axiom_engine_destroy(engine);
    }

    #[test]
    fn test_grid_manipulation_c_abi() {
        let engine = axiom_engine_create();

        let mut cell = TileCell::new(101, 500);
        cell.set_flag(tile_flags::ACTIVE, true);
        let res_set = axiom_grid_set_tile(engine, 10, 20, &cell);
        assert_eq!(res_set, return_code::SUCCESS);

        let mut out_cell = TileCell::empty();
        let res_get = axiom_grid_get_tile(engine, 10, 20, &mut out_cell);
        assert_eq!(res_get, return_code::SUCCESS);
        assert_eq!(out_cell.type_id, 101);
        assert_eq!(out_cell.hp, 500);

        // Push item
        let push_res = axiom_grid_push_item(engine, 10, 20, 5, 10);
        assert_eq!(push_res, return_code::SUCCESS);

        // Check buffer pointer and length
        let buf_ptr = axiom_grid_buffer_ptr(engine);
        let buf_len = axiom_grid_buffer_len(engine);
        assert!(!buf_ptr.is_null());
        assert_eq!(buf_len, DEFAULT_GRID_WIDTH * DEFAULT_GRID_HEIGHT);

        // Pop item
        let pop_res = axiom_grid_pop_item(engine, 10, 20, 5, 4);
        assert_eq!(pop_res, return_code::SUCCESS);

        axiom_engine_destroy(engine);
    }

    #[test]
    fn test_grid_action_and_netcode_c_abi() {
        let engine = axiom_engine_create();

        // 1. Test axiom_grid_apply_action
        let place_act = GridAction::set_tile(0, 15, 15, 200, 800);
        let res_apply = axiom_grid_apply_action(engine, &place_act);
        assert_eq!(res_apply, return_code::SUCCESS);

        let mut cell = TileCell::empty();
        axiom_grid_get_tile(engine, 15, 15, &mut cell);
        assert_eq!(cell.type_id, 200);
        assert_eq!(cell.hp, 800);

        // 2. Test axiom_grid_net lifecycle
        let session0 = axiom_grid_net_create(0);
        let session1 = axiom_grid_net_create(1);
        assert!(!session0.is_null());
        assert!(!session1.is_null());

        // Check grid pointer
        let grid_ptr = axiom_grid_net_get_grid_ptr(session0);
        assert!(!grid_ptr.is_null());

        // Advance peer 0 with local action at frame 0
        let act0 = GridAction::set_tile(0, 10, 10, 50, 100);
        let frame0 = axiom_grid_net_advance_frame(session0, &act0);
        assert_eq!(frame0, 1);

        // Advance peer 1 with empty action at frame 0 and frame 1
        axiom_grid_net_advance_frame(session1, core::ptr::null());
        axiom_grid_net_advance_frame(session1, core::ptr::null());

        // Now deliver peer 0's frame 0 action to peer 1 (delayed!)
        let rolled_back = axiom_grid_net_receive_remote(session1, 0, 0, &act0);
        assert!(rolled_back, "Peer 1 should have rolled back upon receiving delayed action");
        assert!(axiom_grid_net_rollbacks_count(session1) > 0);

        // Advance peer 0 to frame 2 with empty actions
        axiom_grid_net_advance_frame(session0, core::ptr::null());

        // Both peers at frame 2 should have identical state hashes!
        let hash0 = axiom_grid_net_state_hash(session0);
        let hash1 = axiom_grid_net_state_hash(session1);
        assert_eq!(hash0, hash1);

        // Test syncing back to engine context
        let sync_res = axiom_grid_net_sync_to_context(session0, engine);
        assert_eq!(sync_res, return_code::SUCCESS);
        assert_eq!(axiom_engine_tick_count(engine), 2);

        axiom_grid_net_destroy(session0);
        axiom_grid_net_destroy(session1);
        axiom_engine_destroy(engine);
    }
}

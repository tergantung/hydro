#[allow(dead_code)]
pub const DIR_RIGHT: i32 = 3;
#[allow(dead_code)]
pub const DIR_LEFT: i32 = 7;

// Animation IDs from animationID.json (GameAssembly enum)
#[allow(dead_code)]
pub const ANIM_NONE: i32 = 0;
#[allow(dead_code)]
pub const ANIM_IDLE: i32 = 1;
#[allow(dead_code)]
pub const ANIM_WALK: i32 = 2;
#[allow(dead_code)]
pub const ANIM_JUMP: i32 = 3;
#[allow(dead_code)]
pub const ANIM_START_FALL: i32 = 4;
#[allow(dead_code)]
pub const ANIM_FALL: i32 = 5;
#[allow(dead_code)]
pub const ANIM_HIT: i32 = 6;        // Mining swing (stationary)
#[allow(dead_code)]
pub const ANIM_HIT_MOVE: i32 = 7;   // Mining swing while walking
#[allow(dead_code)]
pub const ANIM_PUNCH: i32 = 6;      // Alias for HIT
#[allow(dead_code)]
pub const ANIM_TAKE_HIT: i32 = 67;  // Getting damaged

pub const TILE_WIDTH: f64 = 0.32;
pub const TILE_HEIGHT: f64 = 0.32;

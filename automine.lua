
local MINE_TIERS = {
  { name = "Newbie",   level = 5,  world_id = 0, key_id = 0      },
  { name = "Bronze",   level = 10, world_id = 1, key_id = 0x0f81 },
  { name = "Silver",   level = 20, world_id = 2, key_id = 0x0f82 },
  { name = "Golden",   level = 40, world_id = 3, key_id = 0x0f83 },
  { name = "Platinum", level = 60, world_id = 4, key_id = 0x0f84 },
}

local GEMSTONE_BLOCK_IDS = { 0x0f9b, 0x0f9c, 0x0f9f, 0x0fa0, 0x0fa1, 0x0fa2, 0x0fa3 }

local DEFAULT_GEM_IDS = {

  0x0fac, 0x0fad, 0x0fae, 0x0faf, 0x0fb0,
  0x0fb1, 0x0fb2, 0x0fb3, 0x0fb4, 0x0fb5,
  0x0fb6, 0x0fb7, 0x0fb8, 0x0fb9, 0x0fba,
  0x0fbb, 0x0fbc, 0x0fbd, 0x0fbe, 0x0fbf,
  0x0fc0, 0x0fc1, 0x0fc2, 0x0fc3, 0x0fc4,
  0x0fc5, 0x0fc6, 0x0fc7, 0x0fc8, 0x0fc9,
  0x0fca, 0x0fcb, 0x0fcc, 0x0fcd, 0x0fce,
  0x0fcf, 0x0fd0, 0x0fd1, 0x0fd2, 0x0fd3,
  0x0fd4, 0x0fd5, 0x0fd6, 0x0fd7, 0x0fd8,
}

local PICKAXE_TIERS = {
  { name = "Crappy",  id = 0x0ff7, max_dur = 5000 },
  { name = "Flimsy",  id = 0x0ff8, max_dur = 5500 },
  { name = "Basic",   id = 0x0ff9, max_dur = 6000 },
  { name = "Sturdy",  id = 0x0ffa, max_dur = 6500 },
  { name = "Heavy",   id = 0x0ffb, max_dur = 7000 },
}

local CRYSTAL_BLOCK_IDS = { 3974, 3975, 3976 }

local REPAIR_KIT_BLOCK_ID         = 0x1041
local INV_TYPE_WEAPON_PICKAXES    = 49
local INV_TYPE_CONSUMABLE_REPAIR  = 35

local SPEED_TIMINGS = {
  think  = { punch_ms = 500, walk_settle_ms = 300 },
  normal = { punch_ms = 350, walk_settle_ms = 220 },
  fast   = { punch_ms = 220, walk_settle_ms = 150 },
}

local ACT_DEADLINE_S            = 10
local SURVEY_EMPTY_LIMIT        = 24
local PUNCH_RETRY_CAP           = 10
local BL_GEM_DURATION_S         = 60
local PERIODIC_TICK_S           = 15
local PICKAXE_UPGRADE_INTERVAL_S = 300

local HUMANIZER_CHAT_MIN_S      = 1800
local HUMANIZER_CHAT_MAX_S      = 5400
local HUMANIZER_BREAK_MIN_S     = 3600
local HUMANIZER_BREAK_MAX_S     = 10800
local HUMANIZER_BREAK_IDLE_MIN_S = 120
local HUMANIZER_BREAK_IDLE_MAX_S = 600

local CHAT_LINES = {
  "afk", "brb", "lf gem trades", "buying gemstones",
  "anyone selling pickaxe?", "wts ores cheap", "trading",
  "lf miner", "lol got dced", "back",
}

local INVENTORY_NEAR_FULL = 130

local DIG_COSTS = {
  [3980] = 4,
  [3981] = 4,
  [3982] = 4,
  [3983] = 4,
  [3984] = 4,
  [3989] = 6,
  [3994] = 7,
  [3991] = 9,
}

local DIG_PATH_MAX_VISIT  = 1200

local client = getClient()
if not client then
  print("[automine] no parent client; aborting")
  return
end

local TAG = "[automine|" .. (client.username or client.id:sub(1, 8)) .. "] "

local function log(msg)
  print(TAG .. tostring(msg))
end

local function logf(fmt, ...)
  print(TAG .. string.format(fmt, ...))
end

local function safe_thread(name, fn)
  return runThread(function()
    local ok, err = pcall(fn)
    if not ok then logf("[%s] crashed: %s", name, tostring(err)) end
  end)
end

local function pack_ik24(inv_type, block_id)
  return bit32.bor(bit32.lshift(inv_type, 24), block_id)
end

local function pack_ik16(inv_type, block_id)
  return bit32.bor(bit32.lshift(inv_type, 16), block_id)
end

local function rand_int(a, b)
  return random.integer(a, b)
end

local function rand_range(a, b)
  return random.number(a, b)
end

local function manhattan(a, b)
  return math.abs(a.x - b.x) + math.abs(a.y - b.y)
end

local DEFAULT_CONFIG = {
  target_world          = "MINE",
  target_blocks         = GEMSTONE_BLOCK_IDS,
  speed_mode            = "normal",
  preferred_tier        = nil,
  exit_on_stop          = true,
  auto_recycle          = false,
  recycle_targets       = {},
  recycle_full_amount   = false,
  recycle_min_amount    = 100,
  auto_craft_keys       = true,
  auto_tier_upgrade     = true,
  auto_pickaxe_upgrade  = true,
  auto_seek_crystal     = false,
  crystal_seek_interval = 5,
  disable_in_hole_detection = true,
  humanizer_chat        = true,
  humanizer_session_break = true,
  dry_run               = false,
}

local function validate(cfg)
  assert(type(cfg) == "table", "config must be a table")
  assert(type(cfg.target_blocks) == "table" and #cfg.target_blocks > 0,
         "target_blocks must be non-empty array")
  assert(SPEED_TIMINGS[cfg.speed_mode] ~= nil,
         "speed_mode must be think/normal/fast")
  if cfg.preferred_tier ~= nil then
    assert(type(cfg.preferred_tier) == "number" and cfg.preferred_tier >= 0
             and cfg.preferred_tier <= 4,
           "preferred_tier must be 0..4 or nil")
  end
end

local function load_config()
  local stored = storage:get("automine.cfg")
  local cfg = {}
  for k, v in pairs(DEFAULT_CONFIG) do cfg[k] = v end
  if type(stored) == "table" then
    for k, v in pairs(stored) do cfg[k] = v end
  end
  validate(cfg)
  return cfg
end

local cfg = load_config()
local config_version = storage:get("automine.cfg_version") or 0

local State = {
  SURVEYING   = "surveying",
  APPROACHING = "approaching",
  ACTING      = "acting",
  EXITING     = "exiting",
}

local ctx = {
  state          = State.SURVEYING,
  target         = nil,
  state_deadline = 0,
  idle_count     = 0,
  punch_count    = 0,
  exit_request   = nil,
  terminated     = false,
}

local pickaxe_durability = nil
local admin_in_world     = nil
local pickaxe_broken_count = 0
local last_destroy = {}
local known_drops  = {}
local gem_blacklist = {}
local gems_mined_since_crystal_seek = 0

local target_queue = {}
local target_keys  = {}
local stats = {
  gems_mined = 0,
  hits_sent  = 0,
  repairs    = 0,
  kicks      = 0,
  started_at = os.time(),
}

local recent_disconnects = storage:get("automine.recent_disconnects") or 0

local running = true
local last_restart = 0
local consecutive_fast_restarts = 0

local function tkey(kind, point, id)
  if kind == "drop" then return "drop:" .. tostring(id) end
  return kind .. ":" .. point.x .. "," .. point.y
end

local function queue_add(entry)
  local k = tkey(entry.kind, entry.point, entry.id)
  if target_keys[k] then return end
  target_keys[k]  = true
  entry.key       = k
  entry.added_at  = entry.added_at or os.time()
  table.insert(target_queue, entry)
end

local function queue_remove(key)
  if not target_keys[key] then return end
  target_keys[key] = nil
  for i, e in ipairs(target_queue) do
    if e.key == key then
      table.remove(target_queue, i)
      return
    end
  end
end

client:on("p:AnP", function(doc)
  if doc.U == client.userid then

    if doc.D ~= nil then pickaxe_durability = doc.D end
  end

  if doc.IsAdmin then
    admin_in_world = doc.U
    log("admin detected in world: " .. tostring(doc.UN or doc.U))
    ctx.exit_request = { reason = "admin entered world", rejoin = false }
  end
end)

client:on("p:DB", function(doc)
  if doc.x and doc.y then
    last_destroy[doc.x .. "," .. doc.y] = os.time()
    queue_remove("gem:" .. doc.x .. "," .. doc.y)
  end
end)

client:on("p:nCo", function(doc)
  if doc.id then
    known_drops[doc.id] = {
      id        = doc.id,
      x         = doc.x or 0,
      y         = doc.y or 0,
      block     = doc.bT,
      ts        = os.time(),
    }
    queue_add({
      kind  = "drop",
      point = Vector2i.new(doc.x or 0, doc.y or 0),
      id    = doc.id,
    })
  end
end)

local KERR_REASONS = {
  [1]  = "spam / rate limit",
  [3]  = "world-level error",
  [5]  = "banned",
  [8]  = "pickaxe broken",
  [13] = "world full",
  [15] = "leave-world race",
}
client:on("p:KErr", function(doc)
  local er = doc.ER or 0
  stats.kicks = stats.kicks + 1
  local reason = KERR_REASONS[er] or "unknown"
  logf("KErr ER=%d (%s) â€” full doc: %s", er, reason, json.encode(doc))
  if er == 8 then
    pickaxe_broken_count = pickaxe_broken_count + 1
  elseif er == 1 then
    cfg.punch_interval_ms = math.min((cfg.punch_interval_ms or SPEED_TIMINGS[cfg.speed_mode].punch_ms) * 2, 2000)
    log("ER=1 â†’ bump punch_interval_ms")
  end
end)

client:on("p:OoIP", function(doc)
  logf("OoIP redirect â†’ IP=%s ER=%s WN=%s",
       tostring(doc.IP), tostring(doc.ER), tostring(doc.WN))
end)

client:on("disconnect", function()
  recent_disconnects = recent_disconnects + 1
  storage:set("automine.recent_disconnects", recent_disconnects)
  logf("DISCONNECT (#%d session) â€” last_error: %s",
       recent_disconnects, tostring(client:lastError()))
  if recent_disconnects >= 2 then
    local cur = SPEED_TIMINGS[cfg.speed_mode].punch_ms
    cfg.punch_interval_ms = math.min(cur * 1.5, 1000)
    log("adaptive throttle: " .. tostring(cfg.punch_interval_ms) .. "ms")
  end
end)

safe_thread("dc-decay", function()
  while running do
    sleep(600000)
    if recent_disconnects > 0 then
      recent_disconnects = recent_disconnects - 1
      storage:set("automine.recent_disconnects", recent_disconnects)
    end
  end
end)

local function inventory_count(block_id, inv_type)
  local inv = client:inventory()
  if not inv then return 0 end
  local item = inv:getItem(block_id, inv_type or 0)
  return item and item.amount or 0
end

local function inventory_full()
  local inv = client:inventory()
  return inv and (inv.slots >= INVENTORY_NEAR_FULL) or false
end

local function any_pickaxe_worn()
  for _, p in ipairs(PICKAXE_TIERS) do
    if client:wearing(p.id) then return p end
  end
  return nil
end

local function wear_best_pickaxe()

  for i = #PICKAXE_TIERS, 1, -1 do
    local p = PICKAXE_TIERS[i]
    if client:hasItem(p.id, 1) then
      if not client:wearing(p.id) then
        local err = client:wear(p.id)
        if not err then
          logf("worn pickaxe: %s", p.name)
          pickaxe_broken_count = 0
          return p
        end
      else
        return p
      end
    end
  end
  return nil
end

local function pick_tier(level)
  if cfg.preferred_tier ~= nil then

    local tier = MINE_TIERS[cfg.preferred_tier + 1]
    if tier and level >= tier.level
        and (tier.key_id == 0 or client:hasItem(tier.key_id, 1)) then
      return tier
    end
  end
  for i = #MINE_TIERS, 1, -1 do
    local tier = MINE_TIERS[i]
    if level >= tier.level
        and (tier.key_id == 0 or client:hasItem(tier.key_id, 1)) then
      return tier
    end
  end
  return MINE_TIERS[1]
end

local function preflight()
  if not client:connected() then return "not connected" end
  if (client.level or 0) < MINE_TIERS[1].level then
    return ("level %d < required %d"):format(client.level or 0, MINE_TIERS[1].level)
  end
  if not any_pickaxe_worn() then

    if not wear_best_pickaxe() then return "no pickaxe in inventory" end
  end
  if not client:hasItem(REPAIR_KIT_BLOCK_ID, 1) then
    log("no repair kit in inventory â€” auto-mine will run but won't survive a broken pickaxe")
  end
  return nil
end

local function destroyed_recently(point)
  local k = point.x .. "," .. point.y
  local ts = last_destroy[k]
  return ts and (os.time() - ts) <= 3
end

local function pkey(p) return p.x .. "," .. p.y end

local function build_tile_grid(world)
  local size = world:size()
  local flat = world:tiles()
  if not flat or not size then return nil end
  local w, h = size.x, size.y
  return {
    w = w,
    h = h,

    foreground = function(_, x, y)
      if x < 0 or y < 0 or x >= w or y >= h then return 0xFF end
      local t = flat[y * w + x + 1]
      return t and (t.foreground or 0) or 0xFF
    end,
    water = function(_, x, y)
      if x < 0 or y < 0 or x >= w or y >= h then return 0 end
      local t = flat[y * w + x + 1]
      return t and (t.water or 0) or 0
    end,
  }
end

local function tile_is_safe(fg)
  if fg == 0 then return true end
  if fg == 0xFF then return false end
  local info = getInfo(fg)
  if not info then return false end

  return bit32.band(info.collision or 0, 0x0c) == 0
end

local function dig_cost(grid, x, y)
  local fg = grid:foreground(x, y)
  if fg == 0 then return 1 end
  if not tile_is_safe(fg) then return math.huge end
  local breakable = DIG_COSTS[fg]
  if breakable then return breakable end
  return math.huge
end

local function dig_path(world, start, goal)
  local grid = build_tile_grid(world)
  if not grid then return nil end

  local goal_key = pkey(goal)
  local came   = {}
  local g      = { [pkey(start)] = 0 }
  local open   = { { f = manhattan(start, goal), node = start } }
  local closed = {}
  local visited = 0

  local DIRS = { {1,0}, {-1,0}, {0,1}, {0,-1} }

  while #open > 0 and visited < DIG_PATH_MAX_VISIT do
    table.sort(open, function(a, b) return a.f < b.f end)
    local cur = table.remove(open, 1).node
    local ck  = pkey(cur)
    if closed[ck] then continue end
    closed[ck] = true
    visited = visited + 1

    local cur_cost  = dig_cost(grid, cur.x, cur.y)
    local has_floor = grid:foreground(cur.x, cur.y + 1) ~= 0
                        or grid:water(cur.x, cur.y + 1) ~= 0
    if manhattan(cur, goal) == 1
        and (cur.x == goal.x or cur.y == goal.y)
        and cur_cost ~= math.huge
        and has_floor then
      local steps = {}
      local k = ck
      while came[k] do
        local entry = came[k]
        table.insert(steps, 1, { point = entry.point, action = entry.action })
        k = pkey(entry.from)
      end
      return steps
    end

    for _, d in ipairs(DIRS) do
      local nx, ny = cur.x + d[1], cur.y + d[2]
      local nk = nx .. "," .. ny
      if not closed[nk] and nk ~= goal_key then
        local cost = dig_cost(grid, nx, ny)
        if cost ~= math.huge then
          local n_g = (g[ck] or math.huge) + cost
          if n_g < (g[nk] or math.huge) then
            g[nk] = n_g
            local n_point = Vector2i.new(nx, ny)
            came[nk] = {
              from   = cur,
              action = (cost == 1) and "walk" or "break",
              point  = n_point,
            }
            table.insert(open, { f = n_g + manhattan({x=nx,y=ny}, goal), node = n_point })
          end
        end
      end
    end
  end
  return nil
end

local function execute_dig_path(steps)
  if not steps or #steps == 0 then return true end

  for _, step in ipairs(steps) do
    if not running or ctx.exit_request then return false end

    if step.action == "break" then

      local world = client:world()
      local function tile_fg()
        local t = world and world:tile(step.point)
        return t and (t.foreground or 0) or 0
      end
      local original_fg = tile_fg()
      if original_fg ~= 0 then
        while running and not ctx.exit_request do
          local now_fg = tile_fg()
          if now_fg == 0 then break end
          if now_fg ~= original_fg then
            logf("dig: tile (%d,%d) morphed %dâ†’%d â€” replan",
                 step.point.x, step.point.y, original_fg, now_fg)
            return false
          end
          local pos = client:point()
          if math.abs(pos.x - step.point.x) > 3 or math.abs(pos.y - step.point.y) > 3 then
            logf("dig: bot drifted out of range at (%d,%d) â†’ (%d,%d) â€” replan",
                 step.point.x, step.point.y, pos.x, pos.y)
            return false
          end
          client:hit(step.point)
          stats.hits_sent = stats.hits_sent + 1
          sleep(SPEED_TIMINGS[cfg.speed_mode].punch_ms + rand_int(0, 50))
        end
      end
    end

    if not client:findPath(step.point) then return false end
    while client:pathfinding() and running do sleep(120) end

    local now = client:point()
    if math.abs(now.x - step.point.x) > 1 or math.abs(now.y - step.point.y) > 2 then
      logf("dig: position divergence (expect %d,%d got %d,%d)",
           step.point.x, step.point.y, now.x, now.y)
      return false
    end
  end
  return true
end

local function build_target_set()
  local s = {}
  for _, id in ipairs(cfg.target_blocks) do s[id] = true end
  return s
end

local function tile_safe(world, point)
  local tile = world:tile(point)
  if not tile then return false end
  local fg = tile.foreground or 0
  if fg == 0 then return true end
  local info = getInfo(fg)
  if not info then return false end

  return bit32.band(info.collision or 0, 0x0c) == 0
       and bit32.band(info.collision or 0, 0x1) == 0
end

local function has_floor_below(world, point)
  local below_tile = world:tile(Vector2i.new(point.x, point.y + 1))
  if not below_tile then return false end
  local fg = below_tile.foreground or 0
  local wt = below_tile.water      or 0
  return fg ~= 0 or wt ~= 0
end

local function pick_stand_tile(world, target_point)
  local candidates = {
    Vector2i.new(target_point.x - 1, target_point.y),
    Vector2i.new(target_point.x + 1, target_point.y),
    Vector2i.new(target_point.x,     target_point.y + 1),
    Vector2i.new(target_point.x,     target_point.y - 1),
  }

  for _, c in ipairs(candidates) do
    if client:isWalkable(c) and tile_safe(world, c) and has_floor_below(world, c) then
      return c
    end
  end
  return nil
end

local function bl_active(point)
  local k = point.x .. "," .. point.y
  local exp = gem_blacklist[k]
  if exp and exp > os.time() then return true end
  if exp then gem_blacklist[k] = nil end
  return false
end

local function bl_set(point, secs)
  gem_blacklist[point.x .. "," .. point.y] = os.time() + (secs or BL_GEM_DURATION_S)
end

local function queue_refresh(world, target_set)
  local tiles = world:tiles()
  if not tiles then return end

  local present = {}
  for _, t in ipairs(tiles) do
    if t.foreground and target_set[t.foreground] then
      present["gem:" .. t.point.x .. "," .. t.point.y] =
        { point = t.point, fg = t.foreground }
    end
  end

  local now = os.time()
  local i = 1
  while i <= #target_queue do
    local e = target_queue[i]
    local drop_stale = (e.kind == "drop" and (now - (e.added_at or 0)) > 30)
    local gem_gone   = (e.kind == "gem" and not present[e.key])
    if drop_stale or gem_gone then
      target_keys[e.key] = nil
      table.remove(target_queue, i)
    else
      i = i + 1
    end
  end

  for k, info in pairs(present) do
    if not target_keys[k] then
      target_keys[k] = true
      table.insert(target_queue, {
        kind = "gem", point = info.point, fg = info.fg,
        key = k, added_at = now,
      })
    end
  end
end

local function queue_pick(me)
  local best_drop, best_drop_dist = nil, math.huge
  local best_same,  best_same_dx     = nil, math.huge
  local best_below, best_below_score = nil, math.huge
  local best_above, best_above_score = nil, math.huge
  for _, e in ipairs(target_queue) do
    if not bl_active(e.point) then
      if e.kind == "drop" then
        local d = math.abs(me.x - e.point.x) + math.abs(me.y - e.point.y)
        if d < best_drop_dist then
          best_drop_dist = d
          best_drop = e
        end
      elseif e.kind == "gem" then
        local dx, dy = e.point.x - me.x, e.point.y - me.y
        local adx, ady = math.abs(dx), math.abs(dy)
        if ady <= 1 then
          if adx < best_same_dx then
            best_same_dx = adx
            best_same = e
          end
        elseif dy >= 2 then
          local score = adx + dy * 2
          if score < best_below_score then
            best_below_score = score
            best_below = e
          end
        else
          local score = adx + ady * 5
          if score < best_above_score then
            best_above_score = score
            best_above = e
          end
        end
      end
    end
  end
  if best_drop and best_drop_dist <= 30 then return best_drop end
  return best_same or best_below or best_above
end

local function survey()
  local world = client:world()
  if not world then return end
  local me = client:point()

  local target_set = build_target_set()
  if cfg.auto_seek_crystal
      and gems_mined_since_crystal_seek >= cfg.crystal_seek_interval then
    for _, cid in ipairs(CRYSTAL_BLOCK_IDS) do target_set[cid] = true end
  end

  queue_refresh(world, target_set)
  local pick = queue_pick(me)
  if pick then
    if pick.kind == "drop" then
      ctx.target = { kind = "drop", point = pick.point, id = pick.id }
    else
      ctx.target = { kind = "gem",  point = pick.point, expected_fg = pick.fg }
    end
    ctx.state = State.APPROACHING
    ctx.idle_count = 0
    return
  end

  local enemies = world:enemies()
  for _, e in pairs(enemies) do
    if e.position then
      local p = Vector2i.new(math.floor(e.position.x), math.floor(e.position.y))
      if not bl_active(p) and manhattan(me, p) <= 20 then
        ctx.target = { kind = "mob", point = p, ai_id = e.id }
        ctx.state = State.APPROACHING
        return
      end
    end
  end

  ctx.idle_count = ctx.idle_count + 1
  if ctx.idle_count >= SURVEY_EMPTY_LIMIT then
    ctx.exit_request = { reason = "exhausted scans", rejoin = true }
  end
end

local function gem_still_exists(world, target)
  if target.kind ~= "gem" then return true end
  if not target.expected_fg then return true end
  local tile = world:tile(target.point)
  if not tile then return true end
  local fg = tile.foreground or 0
  return fg == target.expected_fg
end

local NO_PROGRESS_LIMIT = 5

local function approach()
  local target = ctx.target
  if not target then ctx.state = State.SURVEYING; return end

  local world = client:world()
  if not world then ctx.state = State.SURVEYING; return end

  local last_dist = math.huge
  local stuck_iters = 0

  while running and not ctx.exit_request and client:isAlive() do

    world = client:world()
    if not world then sleep(200); continue end

    if not gem_still_exists(world, target) then

      local actual_fg = 0
      local t = world:tile(target.point)
      if t then actual_fg = t.foreground or 0 end
      logf("approach: gem no longer at (%d,%d) â€” fg=%d, blacklist + back to survey",
           target.point.x, target.point.y, actual_fg)
      bl_set(target.point, BL_GEM_DURATION_S)
      ctx.state = State.SURVEYING
      return
    end

    local me = client:point()
    local cur_dist = manhattan(me, target.point)
    if cur_dist <= 1 and tile_safe(world, me) then
      if has_floor_below(world, me) then
        ctx.state = State.ACTING
        ctx.state_deadline = os.time() + ACT_DEADLINE_S
        ctx.punch_count = 0
        return
      end
    end

    if cur_dist >= last_dist then
      stuck_iters = stuck_iters + 1
      if stuck_iters >= NO_PROGRESS_LIMIT then
        logf("approach: no progress for %d iters (dist=%d) â†’ blacklist",
             stuck_iters, cur_dist)
        bl_set(target.point, BL_GEM_DURATION_S)
        ctx.state = State.SURVEYING
        return
      end
    else
      stuck_iters = 0
    end
    last_dist = cur_dist

    local pre_walk_dist = cur_dist
    local stand = pick_stand_tile(world, target.point)
    if stand and client:findPath(stand) then
      while client:pathfinding() and running and not ctx.exit_request do
        sleep(SPEED_TIMINGS[cfg.speed_mode].walk_settle_ms)
      end
    end

    local progressed = manhattan(client:point(), target.point) < pre_walk_dist

    if not progressed then

      local path = dig_path(world, client:point(), target.point)
      if path then
        if not execute_dig_path(path) then
          sleep(800)
        end
      else
        sleep(800)
      end
    end
  end
end

local function act_collect()
  local d = ctx.target
  client:collect(d.id)
  known_drops[d.id] = nil
  queue_remove("drop:" .. tostring(d.id))
  ctx.state = State.SURVEYING
end

local function act_mob()
  local m = ctx.target
  client:hitEnemy(m.point.x, m.point.y, m.ai_id)
  sleep(SPEED_TIMINGS[cfg.speed_mode].punch_ms)
  ctx.state = State.SURVEYING
end

local function act_mine()
  local target = ctx.target
  if not target or not target.point then
    ctx.state = State.SURVEYING
    return
  end
  while ctx.punch_count < PUNCH_RETRY_CAP
      and os.time() < ctx.state_deadline
      and running
      and not ctx.exit_request do
    if destroyed_recently(target.point) then
      gems_mined_since_crystal_seek = gems_mined_since_crystal_seek + 1
      stats.gems_mined = stats.gems_mined + 1
      ctx.state = State.SURVEYING
      return
    end

    local err = client:hit(target.point)
    if err then logf("hit error: %s", tostring(err)) end
    stats.hits_sent = stats.hits_sent + 1
    ctx.punch_count = ctx.punch_count + 1

    sleep(SPEED_TIMINGS[cfg.speed_mode].punch_ms + rand_int(0, 50))

    for did, d in pairs(known_drops) do
      if manhattan(target.point, { x = d.x, y = d.y }) <= 2 then
        client:collect(did)
        known_drops[did] = nil
        queue_remove("drop:" .. tostring(did))
      end
    end
  end

  if ctx.punch_count >= PUNCH_RETRY_CAP then
    bl_set(target.point, BL_GEM_DURATION_S)
  end
  ctx.state = State.SURVEYING
end

local function act()
  if not ctx.target then ctx.state = State.SURVEYING; return end
  if ctx.target.kind == "drop" then return act_collect() end
  if ctx.target.kind == "mob"  then return act_mob() end
  return act_mine()
end

local function find_exit_portal()
  local items = client:findWorldItems({ class = "Portal" })
  for _, it in ipairs(items or {}) do

    if it.class:lower():find("mineexit", 1, true)
        or it.class:lower():find("netherexit", 1, true) then
      return Vector2i.new(it.x, it.y)
    end
  end
  return nil
end

local function exit_phase()
  local req = ctx.exit_request or { reason = "manual", rejoin = false }
  logf("exiting: reason=%s rejoin=%s", req.reason, tostring(req.rejoin))

  local portal = find_exit_portal()
  if portal then
    client:findPath(portal)
    while client:pathfinding() and running do sleep(200) end
    client:enterPortal(portal)
    sleep(1500)
  else
    log("exit: no portal found â†’ leave")
    client:leave()
    sleep(1000)
  end

  if not req.rejoin then
    ctx.terminated = true
    return
  end

  log("decoy hop via warpRandom")
  local landed = client:warpRandom()
  if landed then
    sleep(rand_int(2500, 6000))
  else
    sleep(2000)
  end

  local tier = pick_tier(client.level or 0)
  logf("rejoin: tier=%s lvl=%d", tier.name, client.level or 0)
  local err = client:mines(tier.world_id)
  if err then logf("rejoin failed: %s", tostring(err)) end

  ctx.exit_request = nil
  ctx.state        = State.SURVEYING
  ctx.idle_count   = 0
end

local function dispatch(name, params)
  if cfg.dry_run then
    logf("[DRY] %s %s", name, json.encode(params or {}))
    return
  end
  client:send(name, params)
end

local function repair_pickaxe()
  if not client:hasItem(REPAIR_KIT_BLOCK_ID, 1) then return false end
  local p = any_pickaxe_worn()
  if not p then return false end
  local packed = pack_ik24(INV_TYPE_WEAPON_PICKAXES, p.id)
  dispatch("A", { mG = "MiningPickaxeRepairing", mP = packed })
  stats.repairs = stats.repairs + 1
  log("repair pickaxe â†’ " .. p.name)
  return true
end

local function recycle_gems()
  local inv = client:inventory()
  if not inv then return end
  local targets = (#cfg.recycle_targets > 0) and cfg.recycle_targets or DEFAULT_GEM_IDS

  for _, id in ipairs(targets) do
    local item = inv:getItem(id, 0)
    if item and item.amount >= cfg.recycle_min_amount then
      local amount = cfg.recycle_full_amount
                       and item.amount
                       or (item.amount - cfg.recycle_min_amount)
      if amount > 0 then
        local packed = pack_ik24(0, id)
        dispatch("A", {
          mG = "RecycleMiningGemstone",
          mP = { iK = packed, a = amount },
        })
        logf("recycle: %d Ã— #%d", amount, id)
        sleep(200)
      end
    end
  end
end

local function craft_missing_keys()
  for _, tier in ipairs(MINE_TIERS) do
    if tier.key_id ~= 0 and (client.level or 0) >= tier.level then
      if not client:hasItem(tier.key_id, 1) then
        local packed = pack_ik24(INV_TYPE_CONSUMABLE_REPAIR, tier.key_id)
        dispatch("A", { mG = "CraftMiningGear", mP = packed })
        logf("craft key for tier %s", tier.name)
        sleep(300)
      end
    end
  end
end

local function craft_pickaxe_upgrade()
  local p = any_pickaxe_worn()
  if not p then return end
  local packed = pack_ik24(INV_TYPE_WEAPON_PICKAXES, p.id)
  dispatch("A", { mG = "CraftMiningPickaxeUpgrade", mP = packed })
end

local last_pickaxe_upgrade = 0

local function periodic_tick()

  if pickaxe_durability and pickaxe_durability < 800 then
    repair_pickaxe()
  end

  if not pickaxe_durability and stats.hits_sent > 0 and stats.hits_sent % 80 == 0 then
    repair_pickaxe()
  end
  if cfg.auto_recycle       then recycle_gems() end
  if cfg.auto_craft_keys    then craft_missing_keys() end
  if cfg.auto_pickaxe_upgrade
      and (os.time() - last_pickaxe_upgrade) >= PICKAXE_UPGRADE_INTERVAL_S then
    craft_pickaxe_upgrade()
    last_pickaxe_upgrade = os.time()
  end
end

local function humanizer_chat()
  if not cfg.humanizer_chat then return end
  client:say(random.choice(CHAT_LINES))
end

local main_loop_active = false

local function in_mining_world()
  local w = client:world()
  if not w then return false end
  local name = (w.name or ""):upper()
  local needle = (cfg.target_world or "MINE"):upper()
  return name:find(needle, 1, true) ~= nil
end

local function ensure_in_mine(tier)
  if in_mining_world() then return true end

  for attempt = 1, 5 do
    if not running or not client:isAlive() then return false end
    local err = client:mines(tier.world_id)
    if err then
      sleep(1500)
    elseif in_mining_world() then
      return true
    end
  end
  log("warp failed: tier=" .. tier.name)
  return false
end

local function main_loop()
  if main_loop_active then
    log("main_loop already active â€” skip duplicate spawn")
    return
  end
  main_loop_active = true
  log("main_loop start")

  local err = preflight()
  if err then
    logf("preflight failed: %s", err)
    main_loop_active = false
    return
  end

  wear_best_pickaxe()

  local tier = pick_tier(client.level or 0)
  logf("targeting tier=%s lvl=%d", tier.name, client.level or 0)
  if not ensure_in_mine(tier) then
    log("can't reach mining world â€” abort")
    main_loop_active = false
    return
  end

  local now = os.time
  local next_periodic = now() + PERIODIC_TICK_S
  local next_chat = cfg.humanizer_chat
    and (now() + rand_int(HUMANIZER_CHAT_MIN_S, HUMANIZER_CHAT_MAX_S))
    or math.huge
  local next_break = cfg.humanizer_session_break
    and (now() + rand_int(HUMANIZER_BREAK_MIN_S, HUMANIZER_BREAK_MAX_S))
    or math.huge

  while running and client:isAlive() and not ctx.terminated do

    if not in_mining_world() and ctx.state ~= State.EXITING then
      log("not in mining world mid-loop â€” re-warping")
      if not ensure_in_mine(pick_tier(client.level or 0)) then
        log("re-warp failed â€” terminate loop")
        ctx.terminated = true
        break
      end
    end

    local cur_ver = storage:get("automine.cfg_version") or 0
    if cur_ver ~= config_version then
      log("config reload triggered")
      cfg = load_config()
      config_version = cur_ver
    end

    if pickaxe_broken_count > 0 then
      if pickaxe_broken_count == 1 then
        repair_pickaxe()
        pickaxe_broken_count = 0
      elseif pickaxe_broken_count == 2 then
        local p = wear_best_pickaxe()
        if p then pickaxe_broken_count = 0
        else
          log("ER=8 strike 2 + no spare pickaxe â€” fatal")
          ctx.exit_request = { reason = "no spare pickaxe", rejoin = false }
        end
      else
        log("ER=8 strike 3+ â€” fatal")
        ctx.exit_request = { reason = "ER=8 Ã—3", rejoin = false }
      end
    end

    if inventory_full() and ctx.state ~= State.EXITING then
      log("inventory near full â†’ gate-exit to credit")
      ctx.exit_request = { reason = "inventory full", rejoin = true }
    end

    if now() >= next_periodic then
      safe_thread("periodic", periodic_tick)
      next_periodic = now() + PERIODIC_TICK_S
    end

    if now() >= next_chat then
      safe_thread("chat", humanizer_chat)
      next_chat = now() + rand_int(HUMANIZER_CHAT_MIN_S, HUMANIZER_CHAT_MAX_S)
    end

    if now() >= next_break then
      log("humanizer session break")
      ctx.exit_request = { reason = "session break", rejoin = true }
      next_break = now() + rand_int(HUMANIZER_BREAK_MIN_S, HUMANIZER_BREAK_MAX_S)
    end

    if ctx.exit_request then
      ctx.state = State.EXITING
    end

    local state = ctx.state
    if     state == State.SURVEYING   then survey()
    elseif state == State.APPROACHING then approach()
    elseif state == State.ACTING      then act()
    elseif state == State.EXITING     then exit_phase()
    end

    sleep(50)
  end

  log("main_loop exit (running=" .. tostring(running)
      .. " alive=" .. tostring(client:isAlive())
      .. " terminated=" .. tostring(ctx.terminated) .. ")")
  main_loop_active = false
end

client:on("connect", function()
  if not running then return end
  if ctx.terminated then return end
  if not cfg then return end
  if main_loop_active then
    log("connect event but main_loop already active â€” skip")
    return
  end

  local elapsed = os.time() - last_restart
  if elapsed < 60 then
    consecutive_fast_restarts = consecutive_fast_restarts + 1
    local backoff_s = math.min(60 * (2 ^ math.min(consecutive_fast_restarts, 5)), 600)
    logf("fast-restart backoff %ds", backoff_s)
    sleep(backoff_s * 1000)
  else
    consecutive_fast_restarts = 0
  end
  last_restart = os.time()
  safe_thread("main_loop", main_loop)
end)

safe_thread("telemetry", function()
  while running do
    sleep(60000)
    storage:set("automine.stats", stats)
  end
end)

safe_thread("stats-report", function()
  local last = { gems_mined = 0, hits_sent = 0, repairs = 0, kicks = 0 }
  while running do
    sleep(5000)
    local elapsed = os.time() - stats.started_at
    local d_gems = stats.gems_mined - last.gems_mined
    local d_hits = stats.hits_sent  - last.hits_sent
    local d_rep  = stats.repairs    - last.repairs
    local d_kick = stats.kicks      - last.kicks
    local rate = elapsed > 0 and (stats.gems_mined * 60 / elapsed) or 0
    local q_drops, q_gems = 0, 0
    for _, e in ipairs(target_queue) do
      if e.kind == "drop" then q_drops = q_drops + 1
      elseif e.kind == "gem" then q_gems = q_gems + 1 end
    end
    logf("[stats %ds] gems=%d (+%d, %.1f/min) hits=%d (+%d) repairs=%d (+%d) kicks=%d (+%d) q=%dg/%dd state=%s",
        elapsed, stats.gems_mined, d_gems, rate,
        stats.hits_sent, d_hits, stats.repairs, d_rep,
        stats.kicks, d_kick, q_gems, q_drops, tostring(ctx.state))
    last.gems_mined = stats.gems_mined
    last.hits_sent  = stats.hits_sent
    last.repairs    = stats.repairs
    last.kicks      = stats.kicks
  end
end)

local function bootstrap_connect()
  local st = client:status()
  if st == Status.IN_WORLD or st == Status.MENU_READY then
    return nil
  end
  if st == Status.BANNED or st == Status.KICKED then
    return "bot banned/kicked: " .. tostring(client:lastError())
  end
  if st == Status.DISCONNECTED or st == Status.ERROR
      or st == Status.ALREADY_CONNECTED then
    log("bootstrap: bot not connected â†’ calling connect()")
    local err = client:connect()
    if err then return "connect failed: " .. tostring(err) end
    return nil
  end

  log("bootstrap: handshake in flight (" .. tostring(st) .. ") â€” waiting")
  local deadline = os.time() + 12
  while os.time() < deadline do
    st = client:status()
    if st == Status.IN_WORLD or st == Status.MENU_READY then return nil end
    if st == Status.BANNED or st == Status.KICKED or st == Status.ERROR then
      return "handshake terminal: " .. tostring(client:lastError())
    end
    sleep(300)
  end
  return "bootstrap: handshake timeout"
end

local boot_err = bootstrap_connect()
if boot_err then
  log("can't bootstrap: " .. boot_err)
  return
end

log("bootstrap complete (status=" .. tostring(client:status())
    .. " world=" .. tostring(client:world() and client:world().name)
    .. ") â€” spawning main_loop")
last_restart = os.time()
main_loop()
log("script terminated")
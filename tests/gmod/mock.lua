-- A small stand-in for Garry's Mod's Lua environment, to run portable
-- bundles (`nlc bundle`) on a stock LuaJIT the way gmod would: plain Lua
-- 5.1 plus the globals the NL host uses, with what gmod leaves out
-- removed. It follows the gmod wiki's description of each function, not
-- gmod's code.
--
--   luajit tests/gmod/mock.lua [--server] bundle.lua...
--
-- Loads each bundle in turn with CompileString, as gmod loads Lua files
-- (two addons' bundles share one runtime), then
-- runs frames until nothing is left to do: each frame advances the clock
-- by one tick, runs the timers that are due and then `hook.Run("Think")`.
-- When there is nothing to do but wait for a timer, the clock jumps to it.
-- The run ends when no timer is left and the NL runtime of the state
-- (the global `__nl_runtime` the bundle publishes) has nothing queued and
-- no task pending; a task still pending then is an error, as in `nlc run`.
-- Errors in hooks and timers are reported on stderr as gmod reports them,
-- and the run goes on; the exit status is then 1.
--
-- Environment:
--   SERVER, CLIENT          the realm: client unless --server
--   hook                    Add, Remove, Run, Call, GetTable. Call runs the
--                           handlers in `pairs` order until one returns a
--                           first value that is not nil, returns at most 6
--                           values, then calls the gamemode's function
--   timer                   Simple, Create, Remove, Exists
--   gmod.GetGamemode, GAMEMODE, GM
--   CompileString(code, identifier, handle_error)
--                           the chunk is named "@" .. identifier, so errors
--                           read "identifier:line:"
--   include(path)           runs a Lua file, relative to the working directory
--   AddCSLuaFile            does nothing
--   ErrorNoHalt, ErrorNoHaltWithStack, MsgN, Msg
--   CurTime, RealTime, SysTime, FrameTime
--   isstring, isnumber, isbool, isfunction, istable, IsValid
--   FindMetaTable, Entity(index), Player(userid), Color(r, g, b, a): see
--                           Classes below
--   the string metatable's __index also takes a number: ("abc")[2] == "b"
-- Removed, as gmod removes them (gmod-lua-shared's glua.cpp): loadstring,
-- load, loadfile, dofile, io, ffi, os apart from os.time, os.clock,
-- os.date and os.difftime, debug.setlocal, debug.setupvalue,
-- debug.upvalueid and debug.upvaluejoin, package.path, package.cpath,
-- package.loadlib and package.searchpath, jit.security.
--
-- Each argument is a Lua file run as gmod runs one; the parts of a split
-- bundle are passed as files of their own, in any order.

local args = { ... }
local bundles, realm = {}, "client"
for i = 1, #args do
  if args[i] == "--server" then realm = "server"
  elseif args[i] == "--client" then realm = "client"
  else bundles[#bundles + 1] = args[i] end
end

local real_loadstring, real_require = loadstring, require
local stderr, open, exit = io.stderr, io.open, os.exit
local concat, sort, remove = table.concat, table.sort, table.remove
local select, type, pairs, tostring, pcall, xpcall = select, type, pairs, tostring, pcall, xpcall
local traceback = debug.traceback

if #bundles == 0 then
  stderr:write("usage: luajit mock.lua [--server] bundle.lua...\n")
  exit(2)
end

local function read_file(path)
  local f = open(path, "rb")
  if f == nil then return nil end
  local text = f:read("*a")
  f:close()
  return text
end

local failed = false
local function report(msg)
  failed = true
  stderr:write("[ERROR] " .. tostring(msg) .. "\n")
end

-- Realm --------------------------------------------------------------------

SERVER = realm == "server"
CLIENT = not SERVER

-- Type tests -----------------------------------------------------------------

function isstring(v) return type(v) == "string" end
function isnumber(v) return type(v) == "number" end
function isbool(v) return type(v) == "boolean" end
function isfunction(v) return type(v) == "function" end
function istable(v) return type(v) == "table" end
function IsValid(v)
  if v == nil or v == false then return false end
  local ok, f = pcall(function() return v.IsValid end)
  if not ok or type(f) ~= "function" then return false end
  return f(v) and true or false
end

-- Output and errors -------------------------------------------------------

function Msg(...) stderr:write(concat({ ... })) end
function MsgN(...) stderr:write(concat({ ... }) .. "\n") end
function ErrorNoHalt(...) failed = true; stderr:write(concat({ ... })) end
function ErrorNoHaltWithStack(...) failed = true; stderr:write(concat({ ... }) .. "\n" .. traceback() .. "\n") end

-- Loading code ------------------------------------------------------------

function CompileString(code, identifier, handle_error)
  local f, err = real_loadstring(code, "@" .. tostring(identifier))
  if f ~= nil then return f end
  if handle_error == false then return err end
  report(err)
  return nil
end

function include(path)
  local text = read_file(path)
  if text == nil then
    report("Couldn't include file '" .. tostring(path) .. "' (File not found)")
    return
  end
  local f = CompileString(text, path, false)
  if type(f) ~= "function" then report(f) return end
  return f()
end

function AddCSLuaFile() end

-- gmod's `require` is its own and searches gmod's folders; the mock's is
-- LuaJIT's, searching the working directory, with the search path that
-- gmod's `package` table does not have set only while it runs.
local real_package = package
function require(name)
  if name == "ffi" then error("Module not found!", 2) end
  real_package.path = "./?.lua"
  local ok, result = pcall(real_require, name)
  real_package.path = nil
  if not ok then error(result, 2) end
  return result
end

-- Classes ------------------------------------------------------------------
--
-- A few of gmod's classes, as userdata with the class's metatable, which
-- FindMetaTable returns: Entity, Player : Entity, Weapon : Entity.
-- Entity(index) and Player(userid) give the objects of a small fixed world:
-- players 1 "bob" and 2 "ann", a prop_physics at index 10 and a weapon_crowbar
-- at index 11. Color is a class over tables, as in gmod: Color(r, g, b, a)
-- gives a table with the metatable FindMetaTable("Color") returns.

do
  local metas, data = {}, setmetatable({}, { __mode = "k" })
  local dsetmetatable = debug.setmetatable
  local function class(name, parent)
    local meta = { MetaName = name }
    meta.__index = meta
    meta.__tostring = function(self) return name .. " [" .. data[self].index .. "]" end
    if parent ~= nil then setmetatable(meta, { __index = metas[parent] }) end
    metas[name] = meta
    return meta
  end
  local Entity_ = class("Entity")
  function Entity_:EntIndex() return data[self].index end
  function Entity_:GetClass() return data[self].class end
  function Entity_:IsValid() return true end
  local Player_ = class("Player", "Entity")
  function Player_:Nick() return data[self].nick end
  function Player_:UserID() return data[self].userid end
  class("Weapon", "Entity")

  local COLOR = { MetaName = "Color" }
  COLOR.__index = COLOR
  metas.Color = COLOR
  function Color(r, g, b, a)
    return setmetatable({ r = r, g = g, b = b, a = a or 255 }, COLOR)
  end

  function FindMetaTable(name) return metas[name] end

  local function object(meta, fields)
    local u = newproxy(false)
    dsetmetatable(u, metas[meta])
    data[u] = fields
    return u
  end
  local players = {
    object("Player", { index = 1, userid = 1, class = "player", nick = "bob" }),
    object("Player", { index = 2, userid = 2, class = "player", nick = "ann" }),
  }
  local entities = {
    [1] = players[1], [2] = players[2],
    [10] = object("Entity", { index = 10, class = "prop_physics" }),
    [11] = object("Weapon", { index = 11, class = "weapon_crowbar" }),
  }
  function Player(userid) return players[userid] end
  function Entity(index) return entities[index] end
end

-- The clock --------------------------------------------------------------

local TICK = 1 / 66
local now = 0
function CurTime() return now end
function RealTime() return now end
function SysTime() return now end
function FrameTime() return TICK end

-- The gamemode -------------------------------------------------------------

GAMEMODE = {}
GM = GAMEMODE
gmod = { GetGamemode = function() return GAMEMODE end }

-- hook ------------------------------------------------------------------------

do
  local hooks = {}
  hook = {}

  function hook.GetTable() return hooks end

  function hook.Add(event_name, identifier, func)
    if type(event_name) ~= "string" then
      ErrorNoHaltWithStack("bad argument #1 to 'Add' (string expected, got " .. type(event_name) .. ")")
      return
    end
    if type(func) ~= "function" then
      ErrorNoHaltWithStack("bad argument #3 to 'Add' (function expected, got " .. type(func) .. ")")
      return
    end
    if type(identifier) ~= "string" and not IsValid(identifier) then
      ErrorNoHaltWithStack("bad argument #2 to 'Add' (string expected, got " .. type(identifier) .. ")")
      return
    end
    if hooks[event_name] == nil then hooks[event_name] = {} end
    hooks[event_name][identifier] = func
  end

  function hook.Remove(event_name, identifier)
    if type(event_name) ~= "string" then
      ErrorNoHaltWithStack("bad argument #1 to 'Remove' (string expected, got " .. type(event_name) .. ")")
      return
    end
    local t = hooks[event_name]
    if t ~= nil then t[identifier] = nil end
  end

  function hook.Call(event_name, gm, ...)
    local t = hooks[event_name]
    if t ~= nil then
      for k, v in pairs(t) do
        local a, b, c, d, e, f
        if type(k) == "string" then
          a, b, c, d, e, f = v(...)
        elseif IsValid(k) then
          a, b, c, d, e, f = v(k, ...)
        else
          t[k] = nil
        end
        if a ~= nil then return a, b, c, d, e, f end
      end
    end
    if gm == nil then return end
    local gf = gm[event_name]
    if gf == nil then return end
    return gf(gm, ...)
  end

  function hook.Run(event_name, ...)
    return hook.Call(event_name, gmod.GetGamemode(), ...)
  end
end

-- timer -----------------------------------------------------------------------

local timers = {} -- { due, seq, func, id, delay, reps }
local timer_seq = 0
timer = {}

function timer.Simple(delay, func)
  timer_seq = timer_seq + 1
  timers[#timers + 1] = { due = now + (tonumber(delay) or 0), seq = timer_seq, func = func }
end

function timer.Create(id, delay, reps, func)
  timer.Remove(id)
  timer_seq = timer_seq + 1
  timers[#timers + 1] = { due = now + delay, seq = timer_seq, func = func, id = id, delay = delay, reps = reps }
end

function timer.Remove(id)
  for i = #timers, 1, -1 do
    if timers[i].id == id then remove(timers, i) end
  end
end

function timer.Exists(id)
  for i = 1, #timers do
    if timers[i].id == id then return true end
  end
  return false
end

local function by_due(a, b)
  if a.due ~= b.due then return a.due < b.due end
  return a.seq < b.seq
end

-- Runs the timers due by now, in order; returns whether any ran.
local function run_timers()
  sort(timers, by_due)
  local due = {}
  for i = 1, #timers do
    if timers[i].due <= now then due[#due + 1] = timers[i] end
  end
  if #due == 0 then return false end
  for _, t in ipairs(due) do
    for i = 1, #timers do
      if timers[i] == t then remove(timers, i) break end
    end
    if t.id ~= nil and t.reps ~= 1 then
      if t.reps > 1 then t.reps = t.reps - 1 end
      t.due = now + t.delay
      timers[#timers + 1] = t
    end
    local ok, err = xpcall(t.func, traceback)
    if not ok then report(err) end
  end
  return true
end

-- What gmod leaves out ------------------------------------------------------

do
  local string_meta = getmetatable("")
  local string_lib = string
  local ssub = string.sub
  string_meta.__index = function(s, key)
    local v = string_lib[key]
    if v ~= nil then return v end
    if tonumber(key) then return ssub(s, key, key) end
  end
end

loadstring, load, loadfile, dofile = nil, nil, nil, nil
io = nil
os = { time = os.time, clock = os.clock, date = os.date, difftime = os.difftime }
package.loaded.ffi = nil
package.loaded.io = nil
package.path, package.cpath, package.loadlib, package.searchpath = nil, nil, nil, nil
debug.setlocal, debug.setupvalue, debug.upvalueid, debug.upvaluejoin = nil, nil, nil, nil
if jit ~= nil then jit.security = nil end

-- The run ----------------------------------------------------------------------

-- The driver is a chunk of its own, named as an embedder's chunks are
-- ("=nl …"), and the main chunk ends in a tail call to it, so the NL
-- traces of the bundles leave the mock's frames out as they leave out
-- the engine's in gmod.
local DRIVER = [[
local bundles, read_file, report, run_timers, timers, by_due, advance, TICK, failed, stderr, exit, sort, traceback =
  ...
for _, bundle_path in ipairs(bundles) do
  local text = read_file(bundle_path)
  if text == nil then
    stderr:write("cannot read " .. bundle_path .. "\n")
    exit(2)
  end
  local main = CompileString(text, (bundle_path:gsub("^.*/", "")), false)
  if type(main) ~= "function" then
    stderr:write(tostring(main) .. "\n")
    exit(1)
  end
  local ok, err = xpcall(main, traceback)
  if not ok then
    report(err)
    exit(1)
  end
end

local MAX_FRAMES = 1000000
for _ = 1, MAX_FRAMES do
  local now = advance(TICK)
  local timed = run_timers()
  local ok, err = xpcall(hook.Run, traceback, "Think")
  if not ok then report(err) end
  local rt = rawget(_G, "__nl_runtime")
  local busy = rt ~= nil and rt.pump()
  if not timed and not busy and #timers > 0 then
    -- Nothing to do before the next timer.
    sort(timers, by_due)
    if timers[1].due > now then advance(timers[1].due - TICK - now) end
  elseif not busy and #timers == 0 then
    local pending = rt ~= nil and rt.pending() or 0
    if pending > 0 then
      stderr:write(pending .. " task(s) still pending and nothing left to run\n")
      exit(1)
    end
    exit(failed() and 1 or 0)
  end
end
stderr:write("still running after " .. MAX_FRAMES .. " frames\n")
exit(1)
]]

local function advance(dt)
  now = now + dt
  return now
end
return real_loadstring(DRIVER, "=nl mock")(bundles, read_file, report, run_timers, timers, by_due, advance, TICK,
  function() return failed end, stderr, exit, sort, traceback)

-- Tests of the event library: the rules that the C library of
-- the fork (lib_event.c) and the Lua stand-in (runtime/nl_62_event.lua)
-- must both follow. Part 1 asserts those rules and nothing else, so the
-- file can run against either: on the fork with its luajit, on stock
-- LuaJIT after the runtime chunk installed the stand-in. Part 2 checks
-- properties of the C library alone, and is skipped when `event` is the
-- stand-in.
--
-- tests/vm/run.sh runs it on the fork with the JIT on, off, and forced
-- (-Ohotloop=1).

local passed, failed, skipped = 0, 0, 0
local function test(name, f)
  local ok, err = pcall(f)
  if ok then passed = passed + 1; print("ok - " .. name)
  else failed = failed + 1; print("not ok - " .. name .. ": " .. tostring(err)) end
end
local function eq(a, b, what)
  if a ~= b then error((what or "value") .. ": got " .. tostring(a) .. ", expected " .. tostring(b), 2) end
end
local function raises(f, ...)
  local ok = pcall(f, ...)
  if ok then error("expected an error", 2) end
end
local pack = function(...) return { n = select("#", ...), ... } end

local ev = event
assert(type(ev) == "table", "no global event")
-- The C library's on_run cannot be dumped; the stand-in's can.
local is_c = not pcall(string.dump, ev.on_run)
local n = 0
local function fresh() n = n + 1; return "E" .. n end

-- Part 1: the rules ---------------------------------------------------------

test("event.run, event.on_run and event.remove exist", function()
  eq(type(ev.run), "function"); eq(type(ev.on_run), "function"); eq(type(ev.remove), "function")
end)

test("package.loaded.event is the library when package exists", function()
  if package then eq(package.loaded.event, ev) end
end)

test("on_run and remove return nothing", function()
  local e = fresh()
  eq(select("#", ev.on_run(e, "h", function() end)), 0, "on_run results")
  eq(select("#", ev.on_run(e, "h", function() end)), 0, "on_run (replace) results")
  eq(select("#", ev.remove(e, "h")), 0, "remove results")
  eq(select("#", ev.remove(e, "absent")), 0, "remove of nothing results")
end)

test("names are strings; a number is converted to its string form", function()
  local log = {}
  ev.on_run(7, "h", function() log[#log + 1] = "seven" end)
  ev.run("7")
  ev.run(7)
  eq(table.concat(log, ","), "seven,seven")
  ev.on_run("N", 1, function() log[#log + 1] = "one" end)
  ev.on_run("N", "1", function() log[#log + 1] = "ONE" end)  -- the same handler name
  log = {}
  ev.run("N")
  eq(table.concat(log, ","), "ONE")
  ev.remove("N", 1)
  log = {}
  ev.run("N")
  eq(#log, 0)
  ev.remove(7, "h")
  ev.on_run(2.5, "h", function() log[#log + 1] = "x" end)
  log = {}
  ev.run("2.5")
  eq(#log, 1)
  ev.remove("2.5", "h")
end)

test("any other name is an error", function()
  local f = function() end
  for _, bad in ipairs({ true, {}, f }) do
    raises(ev.run, bad)
    raises(ev.on_run, bad, "h", f)
    raises(ev.on_run, "E", bad, f)
    raises(ev.remove, bad, "h")
    raises(ev.remove, "E", bad)
  end
  raises(ev.run)
  raises(ev.run, nil)
  raises(ev.on_run, nil, "h", f)
  raises(ev.on_run, "E", nil, f)
  raises(ev.remove, "E")
end)

test("the callback is a function, checked when it is added", function()
  local e = fresh()
  raises(ev.on_run, e, "h", 42)
  raises(ev.on_run, e, "h", nil)
  raises(ev.on_run, e, "h", setmetatable({}, { __call = function() return 1 end }))
  eq(select("#", ev.run(e)), 0, "nothing was added")
  ev.on_run(e, "c", print)  -- a C function is a function
  ev.remove(e, "c")
end)

test("handlers run in insertion order; the first that returns ends the run", function()
  local e, log = fresh(), {}
  ev.on_run(e, "a", function(x) log[#log + 1] = "a" .. x end)
  ev.on_run(e, "b", function(x) log[#log + 1] = "b" .. x; return x * 2, "more" end)
  ev.on_run(e, "c", function(x) log[#log + 1] = "c" .. x end)
  local r = pack(ev.run(e, 5))
  eq(r.n, 2); eq(r[1], 10); eq(r[2], "more")
  eq(table.concat(log, " "), "a5 b5")
  eq(select("#", ev.run(fresh())), 0, "no handlers, no results")
end)

test("return nil ends the run with nil; no return does not end it", function()
  local e, log = fresh(), {}
  ev.on_run(e, "a", function() log[#log + 1] = "a" end)
  ev.on_run(e, "b", function() log[#log + 1] = "b"; return nil end)
  ev.on_run(e, "c", function() log[#log + 1] = "c"; return 1 end)
  local r = pack(ev.run(e))
  eq(r.n, 1); eq(r[1], nil)
  eq(table.concat(log), "ab")
  ev.on_run(e, "b", function() log[#log + 1] = "b"; return nil, nil, 3 end)
  r = pack(ev.run(e))
  eq(r.n, 3); eq(r[3], 3)
end)

test("every handler gets the same arguments, nils included", function()
  local e, seen = fresh(), {}
  ev.on_run(e, "a", function(x, y, z) seen[#seen + 1] = tostring(x) .. tostring(y) .. tostring(z); x = "changed" end)
  ev.on_run(e, "b", function(...) seen[#seen + 1] = select("#", ...) .. ":" .. table.concat({ tostring((...)) }) end)
  ev.run(e, "p", nil, "q")
  eq(table.concat(seen, ","), "pnilq,3:p")
end)

test("re-adding a handler name replaces it in place", function()
  local e, log = fresh(), {}
  ev.on_run(e, "a", function() log[#log + 1] = "a1" end)
  ev.on_run(e, "b", function() log[#log + 1] = "b" end)
  ev.on_run(e, "a", function() log[#log + 1] = "a2" end)
  ev.run(e)
  eq(table.concat(log, " "), "a2 b")
end)

test("remove; removing what is not there; re-adding after removal appends", function()
  local e, log = fresh(), {}
  ev.on_run(e, "a", function() log[#log + 1] = "a" end)
  ev.on_run(e, "b", function() log[#log + 1] = "b" end)
  ev.remove(e, "a")
  ev.remove(e, "nope")
  ev.remove(fresh(), "nope")
  ev.run(e)
  ev.on_run(e, "a", function() log[#log + 1] = "A" end)
  ev.run(e)
  eq(table.concat(log, " "), "b b A")
  ev.remove(e, "a"); ev.remove(e, "b")
  log = {}
  ev.run(e)
  eq(#log, 0, "all removed")
  ev.on_run(e, "z", function() log[#log + 1] = "z" end)
  ev.run(e)
  eq(table.concat(log), "z", "an event can be used again after all its handlers left")
end)

test("a handler removed during a run, not yet reached, is not called", function()
  local e, log = fresh(), {}
  ev.on_run(e, "a", function() log[#log + 1] = "a"; ev.remove(e, "c") end)
  ev.on_run(e, "b", function() log[#log + 1] = "b" end)
  ev.on_run(e, "c", function() log[#log + 1] = "c" end)
  ev.run(e)
  ev.run(e)
  eq(table.concat(log, " "), "a b a b")
end)

test("removing the next handlers, and the current one, during a run", function()
  local e, log = fresh(), {}
  ev.on_run(e, "a", function()
    log[#log + 1] = "a"
    ev.remove(e, "a"); ev.remove(e, "b"); ev.remove(e, "c")
  end)
  ev.on_run(e, "b", function() log[#log + 1] = "b" end)
  ev.on_run(e, "c", function() log[#log + 1] = "c" end)
  ev.on_run(e, "d", function() log[#log + 1] = "d" end)
  ev.run(e)
  ev.run(e)
  eq(table.concat(log, " "), "a d d")
end)

test("a handler that removes itself finishes and the run goes on", function()
  local e, log = fresh(), {}
  ev.on_run(e, "a", function() log[#log + 1] = "a"; ev.remove(e, "a") end)
  ev.on_run(e, "b", function() log[#log + 1] = "b" end)
  ev.run(e); ev.run(e)
  eq(table.concat(log, " "), "a b b")
end)

test("the last handler removes itself and a new one is added", function()
  local e, log = fresh(), {}
  ev.on_run(e, "a", function() log[#log + 1] = "a" end)
  ev.on_run(e, "b", function()
    log[#log + 1] = "b"; ev.remove(e, "b")
    ev.on_run(e, "c", function() log[#log + 1] = "c" end)
  end)
  ev.run(e); ev.run(e)
  eq(table.concat(log, " "), "a b a c")
end)

test("a handler added during a run is called from the next run", function()
  local e, log = fresh(), {}
  ev.on_run(e, "a", function()
    log[#log + 1] = "a"
    ev.on_run(e, "new", function() log[#log + 1] = "new" end)
  end)
  ev.on_run(e, "b", function() log[#log + 1] = "b" end)
  ev.run(e)
  ev.run(e)
  eq(table.concat(log, " "), "a b a b new")
end)

test("a handler replaced during a run, not yet reached, runs as replaced", function()
  local e, log = fresh(), {}
  ev.on_run(e, "a", function()
    log[#log + 1] = "a"
    ev.on_run(e, "b", function() log[#log + 1] = "b2" end)
  end)
  ev.on_run(e, "b", function() log[#log + 1] = "b1" end)
  ev.run(e)
  eq(table.concat(log, " "), "a b2")
end)

test("removed and re-added during a run: not called by that run, last afterwards", function()
  local e, log = fresh(), {}
  ev.on_run(e, "a", function()
    log[#log + 1] = "a"
    if #log == 1 then
      ev.remove(e, "b")
      ev.on_run(e, "b", function() log[#log + 1] = "b2" end)
    end
  end)
  ev.on_run(e, "b", function() log[#log + 1] = "b1" end)
  ev.on_run(e, "c", function() log[#log + 1] = "c" end)
  ev.run(e)
  ev.run(e)
  eq(table.concat(log, " "), "a c a c b2")
end)

test("nested runs of the same event, with changes inside", function()
  local e, log, depth = fresh(), {}, 0
  ev.on_run(e, "a", function()
    depth = depth + 1
    log[#log + 1] = "a" .. depth
    if depth == 1 then ev.run(e) end
    if depth == 2 then ev.remove(e, "c"); ev.on_run(e, "d", function() log[#log + 1] = "d" end) end
    depth = depth - 1
  end)
  ev.on_run(e, "b", function() log[#log + 1] = "b" .. depth end)
  ev.on_run(e, "c", function() log[#log + 1] = "c" .. depth end)
  ev.run(e)
  -- The inner run removes c and adds d: neither run calls c; d waits.
  eq(table.concat(log, " "), "a1 a2 b1 b0")
  log = {}
  ev.run(e)
  eq(table.concat(log, " "), "a1 a2 b1 d b0 d")
end)

test("an error unwinds out of a run and leaves the handlers intact", function()
  local e, log = fresh(), {}
  ev.on_run(e, "a", function(fail)
    log[#log + 1] = "a"
    ev.on_run(e, "late", function() log[#log + 1] = "late" end)
    if fail then error("handler failed") end
  end)
  ev.on_run(e, "b", function() log[#log + 1] = "b" end)
  local ok, err = pcall(ev.run, e, true)
  eq(ok, false)
  eq(tostring(err):find("handler failed") ~= nil, true)
  ev.run(e, false)
  eq(table.concat(log, " "), "a a b late")
end)

test("a handler can yield: the run goes on when the coroutine resumes", function()
  local e, log = fresh(), {}
  ev.on_run(e, "a", function(x) log[#log + 1] = "a"; local v = coroutine.yield("waiting"); log[#log + 1] = "a" .. v end)
  ev.on_run(e, "b", function(x) log[#log + 1] = "b"; return x + coroutine.yield("again") end)
  local co = coroutine.create(function() return ev.run(e, 40) end)
  local ok, w = coroutine.resume(co)
  eq(ok, true); eq(w, "waiting")
  ok, w = coroutine.resume(co, "!")
  eq(ok, true); eq(w, "again")
  local r
  ok, r = coroutine.resume(co, 2)
  eq(ok, true); eq(r, 42)
  eq(coroutine.status(co), "dead")
  eq(table.concat(log, " "), "a a! b")
end)

test("many arguments: no limit but the VM's", function()
  local e, got = fresh(), nil
  ev.on_run(e, "h", function(...) got = select("#", ...) end)
  local t = {}
  for i = 1, 7000 do t[i] = i end
  ev.run(e, unpack(t, 1, 7000))
  eq(got, 7000)
end)

test("deep nesting: no limit but the VM's, and its error names the cause", function()
  local e, depth = fresh(), 0
  ev.on_run(e, "h", function(k)
    if k > 0 then depth = depth + 1; return ev.run(e, k - 1) end
    return "bottom"
  end)
  eq(ev.run(e, 5000), "bottom")
  eq(depth, 5000)
  local d2 = fresh()
  ev.on_run(d2, "h", function() return ev.run(d2) end)
  local ok, err = pcall(ev.run, d2)
  eq(ok, false)
  eq(tostring(err):find("stack overflow") ~= nil, true, "message: " .. tostring(err))
  eq(tostring(err):find("argument") == nil, true, "message: " .. tostring(err))
end)

test("many runs in a hot loop", function()
  local e, sum = fresh(), 0
  ev.on_run(e, "acc", function(x) sum = sum + x end)
  ev.on_run(e, "ret", function(x) if x % 7 == 0 then return x end end)
  local hits = 0
  for i = 1, 20000 do
    if ev.run(e, i) then hits = hits + 1 end
  end
  eq(sum, 20000 * 20001 / 2)
  eq(hits, math.floor(20000 / 7))
end)

-- A model of the rules, and a randomized comparison: handlers that add,
-- replace and remove handlers (themselves included) while runs are in
-- progress, nested runs, returns of values and of nil, and errors.
test("randomized changes during runs agree with the model", function()
  local seed = tonumber(os.getenv("NL_EVENT_SEED") or "") or 12345
  math.randomseed(seed)
  local NAMES = { "h1", "h2", "h3", "h4", "h5", "h6" }

  -- Model: a list of entries {name=, f=, dead=}; an add builds a new list,
  -- so a run in progress keeps the list it began with.
  local M = { list = {}, byname = {} }
  local function m_add(name, f)
    local e = M.byname[name]
    if e then e.f = f; return end
    local nl = {}
    for _, x in ipairs(M.list) do if not x.dead then nl[#nl + 1] = x end end
    e = { name = name, f = f }
    nl[#nl + 1] = e
    M.list, M.byname[name] = nl, e
  end
  local function m_remove(name)
    local e = M.byname[name]
    if e then e.dead = true; M.byname[name] = nil end
  end
  local function m_run(...)
    local list = M.list
    for i = 1, #list do
      local x = list[i]
      if not x.dead then
        local r = pack(x.f(...))
        if r.n > 0 then return unpack(r, 1, r.n) end
      end
    end
  end

  local e = fresh()
  local depth = 0
  local function make(id)
    local plan = {}
    for _ = 1, math.random(0, 3) do
      local k = math.random(1, 11)
      local name = NAMES[math.random(#NAMES)]
      if k <= 3 then plan[#plan + 1] = { "add", name, math.random(1, 1000000) }
      elseif k <= 6 then plan[#plan + 1] = { "remove", name }
      elseif k == 7 then plan[#plan + 1] = { "nest" }
      elseif k == 8 then plan[#plan + 1] = { "return" }
      elseif k == 9 then plan[#plan + 1] = { "return nil" }
      elseif k == 10 then plan[#plan + 1] = { "error" }
      end
    end
    return plan
  end
  local plans = {}
  local function handler(id, log, add, remove, run)
    return function(arg)
      log[#log + 1] = id .. ":" .. tostring(arg)
      for _, step in ipairs(plans[id]) do
        if step[1] == "add" then
          local nid = step[3]
          plans[nid] = plans[nid] or make(nid)
          add(step[2], nid)
        elseif step[1] == "remove" then remove(step[2])
        elseif step[1] == "nest" and depth < 3 then
          depth = depth + 1
          local r = pack(pcall(run, arg + 1))
          log[#log + 1] = "nested:" .. r.n .. ":" .. tostring(r[2])
          depth = depth - 1
        elseif step[1] == "return" then return id
        elseif step[1] == "return nil" then return nil
        elseif step[1] == "error" and depth > 0 then error("planned", 0)
        end
      end
    end
  end
  local rlog, mlog = {}, {}
  local function r_add(name, id) ev.on_run(e, name, handler(id, rlog, r_add, function(nm) ev.remove(e, nm) end, function(a) return ev.run(e, a) end)) end
  local function mm_add(name, id) m_add(name, handler(id, mlog, mm_add, m_remove, m_run)) end
  local nilreturns = 0
  for round = 1, 400 do
    local k = math.random(1, 4)
    local name = NAMES[math.random(#NAMES)]
    if k == 1 then
      local id = round
      plans[id] = make(id)
      r_add(name, id); mm_add(name, id)
    elseif k == 2 then
      ev.remove(e, name); m_remove(name)
    else
      local a = pack(pcall(ev.run, e, round * 10))
      local b = pack(pcall(m_run, round * 10))
      eq(a.n, b.n, "result count, round " .. round)
      for i = 1, a.n do eq(a[i], b[i], "result " .. i .. ", round " .. round) end
      if a.n == 2 and a[1] == true and a[2] == nil then nilreturns = nilreturns + 1 end
    end
    eq(#rlog, #mlog, "calls so far, round " .. round)
  end
  for i = 1, #rlog do eq(rlog[i], mlog[i], "call " .. i) end
  if #rlog < 200 then error("too few handler calls to mean anything: " .. #rlog) end
  if nilreturns == 0 then error("no run ended with `return nil`: the model's case is not covered") end
end)

-- Part 2: the C library alone -----------------------------------------------

local function ctest(name, f)
  if is_c then test("C: " .. name, f)
  else skipped = skipped + 1; print("skip - C: " .. name .. " (event is the stand-in)") end
end

-- The store, found through the registry: a table of records with `byname`.
local function find_store()
  for _, v in pairs(debug.getregistry()) do
    if type(v) == "table" then
      for _, rec in pairs(v) do
        if type(rec) == "table" and type(rawget(rec, "byname")) == "table" then return v end
      end
    end
  end
end

ctest("removed handlers and empty events are released", function()
  collectgarbage(); collectgarbage()
  local before = collectgarbage("count")
  local f = function() end
  for i = 1, 20000 do ev.on_run("Many", "h" .. i, f) end
  for i = 1, 20000 do ev.remove("Many", "h" .. i) end
  for i = 1, 20000 do ev.on_run("Ev" .. i, "h", f) end
  for i = 1, 20000 do ev.remove("Ev" .. i, "h") end
  collectgarbage(); collectgarbage()
  local after = collectgarbage("count")
  -- The 40,000 handler and event names stay interned until collected; the
  -- entries and records (several MB) must be gone.
  if after - before > 1024 then error(string.format("%.0f KB kept", after - before)) end
  local store = find_store()
  if store then eq(rawget(store, "Many"), nil, "record of an empty event") end
end)

ctest("adding does not copy the list", function()
  local f = function() end
  local t0 = os.clock()
  for i = 1, 20000 do ev.on_run("Big", "h" .. i, f) end
  local dt = os.clock() - t0
  for i = 1, 20000 do ev.remove("Big", "h" .. i) end
  if dt > 0.5 then error(string.format("20,000 adds took %.2f s", dt)) end
end)

ctest("a damaged store gives an error, not a crash", function()
  local e = fresh()
  ev.on_run(e, "h", function() return 1 end)
  local store = find_store()
  assert(store, "store not found")
  local rec = rawget(store, e)
  rawset(rec, "head", 5)
  raises(ev.run, e)
  rawset(rec, "byname", 5)
  raises(ev.on_run, e, "h2", function() end)
  raises(ev.remove, e, "h")
  rawset(store, e, 5)
  raises(ev.run, e)
  raises(ev.on_run, e, "h", function() end)
  raises(ev.remove, e, "h")
  rawset(store, e, nil)
  ev.on_run(e, "h", function() return 2 end)
  eq(ev.run(e), 2, "the event works again")
end)

ctest("the library's internal functions refuse to be called from Lua", function()
  for k, v in pairs(debug.getregistry()) do
    if type(v) == "function" and v ~= ev.run then
      pcall(v, io.stdout)
      pcall(v)
    end
  end
end)

print(string.format("# test_events.lua (%s, %s): %d passed, %d failed, %d skipped",
  is_c and "C library" or "stand-in",
  jit and (jit.status() and "JIT on" or "JIT off") or "?", passed, failed, skipped))
os.exit(failed == 0 and 0 or 1)

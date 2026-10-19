-- Events. The fork opens the C `event` library before this
-- chunk runs. On stock LuaJIT the global `event` is absent and this
-- file installs a Lua stand-in with the same three functions, which
-- behaves as the C library does (the same programs run on both):
--
--   event.run(name, ...)            calls the handlers in the order they were
--                                   added; the first that returns a value
--                                   (nil included) ends the run, and its
--                                   values are returned
--   event.on_run(name, handler, f)  adds f, or replaces the function of the
--                                   handler of that name in place (it
--                                   keeps its place); returns nothing
--   event.remove(name, handler)     returns nothing
--
-- Event names and handler names are strings; a number stands for its
-- string form, and any other value raises "bad argument" as the C library
-- does. The function must be a function (a callable table is rejected),
-- checked when it is added. Handlers are called directly, so one may
-- yield: a plain handler that awaits works when `event.run` is called in
-- a task. A removed handler's entry is released, and so is an event's
-- record when it has no handlers left. The stand-in is also
-- `package.loaded.event`.
--
-- Intrinsic: rt.on, what `on` compiles to. The library table: rt.Event.
do
  local function pack(...) return { n = select("#", ...), ... } end
  local ev = rawget(_G, "event")
  if ev == nil then
    -- The store has the shape the C library gives it (lib_event.c), and
    -- `run` is the loop of lj_nlevent.lua, so the two behave alike:
    --   lists[name] = rec   rec.head, rec.tail: the first and last entry
    --                       rec.byname: handler name -> entry
    --                       rec.seq: the sequence number of the last add
    --   entry               e.f: the function; e.seq: when it was added;
    --                       e.next, e.prev: links; e.dead: true once removed
    --                       (e.next is kept, so a run standing on it goes on)
    local lists = {}
    ev = {}

    -- A name argument as the C library reads it (luaL_checkstring). The
    -- error names the caller of the library function.
    local function checkname(v, n, fname, nargs)
      local tv = type(v)
      if tv == "string" then return v end
      if tv == "number" then return tostring(v) end
      error("bad argument #" .. n .. " to '" .. fname .. "' (string expected, got "
        .. (n > nargs and "no value" or tv) .. ")", 3)
    end

    function ev.on_run(...)
      local nargs = select("#", ...)
      local name, handler, f = ...
      name = checkname(name, 1, "on_run", nargs)
      handler = checkname(handler, 2, "on_run", nargs)
      if type(f) ~= "function" then
        error("bad argument #3 to 'on_run' (function expected, got " .. (nargs < 3 and "no value" or type(f)) .. ")", 2)
      end
      local rec = lists[name]
      if rec == nil then
        rec = { byname = {}, seq = 0 }
        lists[name] = rec
      end
      local e = rec.byname[handler]
      if e ~= nil then
        -- Re-adding a handler name replaces its function in place.
        e.f = f
        return
      end
      rec.seq = rec.seq + 1
      e = { f = f, seq = rec.seq, prev = rec.tail }
      if rec.tail ~= nil then rec.tail.next = e else rec.head = e end
      rec.tail = e
      rec.byname[handler] = e
    end

    function ev.remove(...)
      local nargs = select("#", ...)
      local name, handler = ...
      name = checkname(name, 1, "remove", nargs)
      handler = checkname(handler, 2, "remove", nargs)
      local rec = lists[name]
      if rec == nil then return end
      local e = rec.byname[handler]
      if e == nil then return end
      rec.byname[handler] = nil
      if e.prev ~= nil then e.prev.next = e.next else rec.head = e.next end
      if e.next ~= nil then e.next.prev = e.prev else rec.tail = e.prev end
      -- A run in progress skips it; nothing keeps its function.
      e.dead = true
      e.f = nil
      e.prev = nil
      if rec.head == nil then lists[name] = nil end
    end

    local walk

    -- A handler that returns any value, `nil` included, ends the
    -- run, and its values are the run's; one that returns nothing lets the
    -- next handler run.
    local function after(e, max, args, ...)
      if select("#", ...) > 0 then return ... end
      return walk(e.next, max, args)
    end

    -- Calls the entries from e on. One removed since the run began is
    -- skipped; one added since (seq > max; entries are in seq order) ends
    -- the walk; one replaced since runs with its new function.
    walk = function(e, max, args)
      while e ~= nil do
        if e.seq > max then return end
        if not e.dead then
          return after(e, max, args, e.f(unpack(args, 1, args.n)))
        end
        e = e.next
      end
    end

    function ev.run(...)
      local name = checkname((...), 1, "run", select("#", ...))
      local rec = lists[name]
      if rec == nil or rec.head == nil then return end
      return walk(rec.head, rec.seq, pack(select(2, ...)))
    end
    rawset(_G, "event", ev)
    if type(package) == "table" and type(package.loaded) == "table" then
      package.loaded.event = ev
    end
  end
  rt.Event = ev
  -- Kept here, so that a script that replaces `event.on_run` does not
  -- change what `on` does.
  local on_run = ev.on_run

  -- `on Name [as 'handler'] [async] do … end`: adds `f` with
  -- `event.on_run` and returns the handler name, the value of `on`. The
  -- compiler passes the event name as a string and, for an unnamed
  -- handler, the file's canonical name as the handler name; `f` already
  -- discards its tail value.
  --
  -- An async handler starts its task and returns nothing, so the
  -- task is not `Event.run`'s value and the run goes on to the next
  -- handler; a `return` in an async handler gives the task's value only.
  -- A failure of that task that nothing handles goes to the unhandled
  -- hook, as for any task.
  function rt.on(name, handler, f, async)
    local cb = f
    if async then
      local af = rt.async(f)
      cb = function(...) af(...) end
    end
    on_run(name, handler, cb)
    return handler
  end
end

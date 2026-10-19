-- Workers.
--
-- On the fork the host table has `host.worker`, the C side
-- (LuaJIT/src/lj_nlworker.c, lj_nltransfer.c): worker states with threads
-- of their own, the pool's job queue, and the transfer encoder. This file
-- is the Lua side of it: what crosses (the rules the encoder asks about),
-- the bookkeeping of jobs and calls, the library `Worker`, and the
-- functions the worker loop calls (rt.workerstart, rt.workerjob,
-- rt.workerin, rt.workerstep, rt.workercount).
--
-- On stock LuaJIT (`nlc run`, the oracle) there is no `host.worker`, and
-- `spawn` is a stand-in: it runs the job in the same state on deep copies
-- of its arguments. It follows the copy rules of workers, but top-level variables, globals and handlers are this
-- state's, and nothing runs in parallel. Worker.run and Worker.map use it;
-- Worker.new needs the worker runtime and raises.
--
-- Intrinsics: rt.register (the name registry), rt.spawn.
-- Fixed name: rt.settransfer (nl_settransfer). The library table:
-- rt.Worker (`Worker`).
do
  local Worker = {}
  rt.Worker = Worker
  local HW = type(host) == "table" and type(host.worker) == "table" and host.worker or nil

  ---------------------------------------------------------------------------
  -- The name registry: what each loaded file declares under
  -- its canonical name. Named functions and structs declared at file
  -- level or in a module body, under their path of modules (`M.N.f`); the
  -- compiler leaves out what has no name unique in the file
  -- (crates/nl_ir/src/lower/spawn.rs). The hidden function of a `spawn do`
  -- block is registered when its file loads (lift.rs), so a worker that
  -- loads the file finds it by name.

  -- value -> { file, name }
  local named = setmetatable({}, { __mode = "k" })
  I.named = named
  -- file -> name -> value
  local by_file = {}
  I.registry = by_file

  -- Registers `v` as `name` of `file` and returns `v`. Loading a file
  -- again registers its new values under the same names.
  function rt.register(file, name, v)
    named[v] = { file, name }
    local names = by_file[file]
    if names == nil then
      names = {}
      by_file[file] = names
    end
    names[name] = v
    return v
  end

  -- The structs of the runtime library (the built-in exceptions).
  for _, v in pairs(rt) do
    local info = type(v) == "table" and I.info[v] or nil
    if info and info.kind == "struct" and info.file == "=nl runtime" then
      rt.register("=nl runtime", info.name, v)
    end
  end

  ---------------------------------------------------------------------------
  -- What crosses besides structs and named functions.

  -- Worker.register(name, metatable): tables with that metatable cross;
  -- every state that sends or receives them registers the same name.
  local reg_by_name = {}
  local reg_by_meta = setmetatable({}, { __mode = "k" })

  function Worker.register(name, mt)
    if type(name) ~= "string" then rt.typeerror("argument 1 of Worker.register", rt.T.String, name) end
    if type(mt) ~= "table" then rt.typeerror("argument 2 of Worker.register", rt.T.Table, mt) end
    local prev = reg_by_name[name]
    if prev ~= nil and prev ~= mt then reg_by_meta[prev] = nil end
    reg_by_name[name] = mt
    reg_by_meta[mt] = name
  end

  -- The transfer functions of host types (nl_settransfer):
  -- name -> { encode, decode }. A Regex crosses as its pattern and is
  -- compiled again on arrival (through the receiver's pattern cache).
  local transfer = {}
  function rt.settransfer(name, encode, decode)
    if type(name) ~= "string" then error("settransfer: the type name must be a string", 2) end
    if type(encode) ~= "function" or type(decode) ~= "function" then
      error("settransfer: encode and decode must be functions", 2)
    end
    transfer[name] = { encode, decode }
  end
  transfer.Regex = {
    function(re) return re.pattern end,
    function(pattern) return rt.Regex.new(pattern) end,
  }

  ---------------------------------------------------------------------------
  -- SendError messages: the value, the part of the message it
  -- is in (`argument 2`, `self`, `captured \`x\``, `callee`, `result`,
  -- `exception`), and where inside that part (`.list[2]`).

  local function key_text(k)
    if type(k) == "string" then
      if smatch(k, "^[%a_][%w_]*$") then return "." .. k end
      return "[" .. sformat("%q", k) .. "]"
    elseif type(k) == "number" or type(k) == "boolean" then
      return "[" .. tostring(k) .. "]"
    end
    return "[" .. tostring(rt.typeof(k)) .. " key]"
  end

  local function send_error(what, part, path)
    local where = part
    if path ~= "" then where = where .. ", at " .. path end
    I.throw("SendError", "cannot send " .. what .. " (" .. where .. ")")
  end

  -- A place in a message: nil for the value itself, or { parent, key }.
  local function path_text(place)
    local keys = {}
    while place ~= nil do
      keys[#keys + 1] = place[2]
      place = place[1]
    end
    local parts = {}
    for i = #keys, 1, -1 do parts[#parts + 1] = key_text(keys[i]) end
    return tconcat(parts)
  end

  -- What a struct without a registered name is.
  local NO_NAME = "which has no name another state can find (it is declared inside a function or in a nested block,"
    .. " or another declaration of its module has its name)"

  -- The callee of a job: a named function, else SendError.
  local function check_callee(f, part)
    if type(f) ~= "function" then
      send_error("a value of type " .. tostring(rt.typeof(f)) .. " as the function to run", part, "")
    end
    if not named[f] then send_error("a function without a name", part, "") end
  end

  -- "file:line" of the code that called into the runtime: the first frame
  -- past C functions and the chunks of the runtime and the embedder (whose
  -- names start with "=nl " or "=[nl ", as for traces, nl_50_error.lua).
  local getinfo = ldebug.getinfo
  local function caller_position()
    if getinfo ~= nil then
      for level = 3, 40 do
        local info = getinfo(level, "Sl")
        if info == nil then break end
        local src = info.source
        if info.what ~= "C" and type(src) == "string" and ssub(src, 1, 4) ~= "=nl "
           and ssub(src, 1, 5) ~= "=[nl " then
          return tostring(info.short_src) .. ":" .. tostring(info.currentline)
        end
      end
      return nil
    end
    local ok, msg = pcall(error, "", 4)
    if not ok and type(msg) == "string" and msg ~= "" then return (sgsub(msg, ":%s*$", "")) end
    return nil
  end

  -- The canonical name of the file whose code called into the runtime.
  local function caller_file()
    if getinfo == nil then return nil end
    for level = 3, 40 do
      local info = getinfo(level, "S")
      if info == nil then break end
      local src = info.source
      if type(src) == "string" and ssub(src, 1, 1) == "@" then return ssub(src, 2) end
    end
    return nil
  end

  -- What `spawn` is called with (the four forms the compiler emits, below):
  -- the function, its arguments, their count and their labels.
  --
  --   rt.spawn(nil, f, args...)              spawn f(args)
  --   rt.spawn(false, f, m, args...)         spawn f(args) of a module function
  --                                          called by its bare name in its
  --                                          module: the module m is `self`
  --   rt.spawn("m", x, args...)              spawn x.m(args): the member-call
  --                                          rule decides whether x is `self`
  --   rt.spawn({file, name, captures...}, f, values...)
  --                                          spawn do … end: f is the block as
  --                                          a hidden named function whose
  --                                          parameters are the captured
  --                                          locals, named in the table
  --
  -- The written arguments are numbered from 1 in every form; a
  -- receiver or module passed as `self` is labelled `self`.
  local function spawn_parts(how, x, ...)
    local n = select("#", ...)
    local f, args, labels = nil, { ... }, nil
    if how == nil then
      f = x
    elseif how == false then
      f = x
      labels = { "self" }
    elseif type(how) == "string" then
      local m
      f, m = I.lookup(x, how, 3)
      if m and type(f) == "function" then
        args = { x, ... }
        n = n + 1
        labels = { "self" }
      end
    else
      -- Registered when the file loaded (lift.rs; a block it cannot make
      -- then is a compile error).
      f = x
      labels = {}
      for i = 1, n do labels[i] = "captured `" .. tostring(how[i + 2]) .. "`" end
    end
    local offset = (labels and labels[1] == "self") and 1 or 0
    local parts = {}
    for i = 1, n do parts[i] = labels and labels[i] or ("argument " .. (i - offset)) end
    return f, args, n, parts
  end

  -- The batch of Worker.map: f(item, extra...) for each item of the batch.
  -- Registered in every state under the runtime's own name.
  local function map_batch(f, items, count, ...)
    local out = {}
    local is_async = I.async_fns[f]
    for i = 1, count do
      local r = f(items[i], ...)
      if is_async then r = rt.Task.await(r) end
      out[i] = r
    end
    return out
  end
  rt.register("=nl runtime", "Worker.map batch", map_batch)

  ---------------------------------------------------------------------------
  -- Worker.map: the items in batches, so that small work does not pay for
  -- a job each. About four batches per pool worker.

  local function map_with(spawn_fn, list, f, ...)
    if type(list) ~= "table" then rt.typeerror("argument 1 of Worker.map", rt.T.Table, list) end
    check_callee(f, "argument 2")
    local n = #list
    local out = rt.newtask()
    if n == 0 then
      I.finish(out, "done", {})
      return out
    end
    local workers = Worker.count
    if type(workers) ~= "number" or workers < 1 then workers = 1 end
    local nbatches = n < workers * 4 and n or workers * 4
    local size = floor((n + nbatches - 1) / nbatches)
    local tasks, firsts = {}, {}
    local i = 1
    while i <= n do
      local last = i + size - 1
      if last > n then last = n end
      local items = {}
      for k = i, last do items[k - i + 1] = list[k] end
      tasks[#tasks + 1] = spawn_fn(map_batch, f, items, last - i + 1, ...)
      firsts[#firsts + 1] = i
      i = last + 1
    end
    local all = rt.Task.all(tasks)
    rawset(all, "_observed", true)
    I.when_done(all, function(state, v)
      if state ~= "done" then
        I.finish(out, state, v)
        return
      end
      local result = {}
      for b = 1, #tasks do
        local part, first = v[b], firsts[b]
        local count = (b < #tasks and firsts[b + 1] or n + 1) - first
        for k = 1, count do result[first + k - 1] = part[k] end
      end
      I.finish(out, "done", result)
    end)
    return out
  end

  if HW == nil then
    -------------------------------------------------------------------------
    -- The stand-in (stock LuaJIT).

    Worker.count = 0
    local function m10(name)
      return function()
        error("NL runtime: " .. name .. " needs the worker runtime of the NL VM, which stock LuaJIT does not have", 2)
      end
    end
    Worker.new = m10("Worker.new")

    -- The copy of `v` as part `part` of a message. One
    -- message is copied with one `seen` table, so shared references and
    -- cycles keep their shape within it. The copy keeps its own stack of
    -- tables to fill instead of recursing, so a structure of any depth or
    -- length is copied (or refused with SendError) without overflowing
    -- Lua's stack; the path that names where a value is (`.list[2]`) is
    -- built only for the message.
    local function copy(v, seen, part)
      -- Tables whose entries are still to copy: { source, copy, last key, place }.
      local stack = {}
      -- The copy of `x`, found at `place`. A new table is created empty and
      -- filled later from the stack.
      local function shell(x, place)
        local tv = type(x)
        if tv == "nil" or tv == "boolean" or tv == "number" or tv == "string" then return x end
        if tv == "function" then
          -- A named function arrives as the receiver's own function of that
          -- name: in this state, the same one.
          if named[x] then return x end
          send_error("a function without a name", part, path_text(place))
        elseif tv == "thread" then
          send_error("a coroutine", part, path_text(place))
        elseif tv ~= "table" then
          -- Userdata: no host type has transfer functions in the stand-in.
          send_error("a value of type " .. tostring(rt.typeof(x)), part, path_text(place))
        end
        local done = seen[x]
        if done ~= nil then return done end
        -- A type itself (a struct, enum, module or interface) crosses by name.
        -- Only a type with a registered name, as on the fork.
        local info = I.info[x]
        if info then
          if named[x] or info.kind == "builtin" then return x end
          send_error((info.kind == "struct" and "struct " or ("the " .. tostring(info.kind) .. " "))
            .. tostring(info.name) .. ", " .. NO_NAME, part, path_text(place))
        end
        local mt = getmetatable(x)
        local out
        if mt == nil then
          out = {}
        elseif mt == I.Range then
          out = setmetatable({}, mt)
        elseif mt == I.Task then
          send_error("a task", part, path_text(place))
        elseif reg_by_meta[mt] then
          out = setmetatable({}, mt)
        else
          local minfo = I.info[mt]
          if minfo and minfo.kind == "struct" then
            if not named[mt] then
              send_error("an instance of struct " .. tostring(minfo.name) .. ", " .. NO_NAME, part, path_text(place))
            end
            -- The same struct; types are not checked again.
            out = setmetatable({}, mt)
          else
            send_error("a table with a metatable that is not a struct", part, path_text(place))
          end
        end
        seen[x] = out
        stack[#stack + 1] = { x, out, nil, place }
        return out
      end
      local result = shell(v, nil)
      while true do
        local n = #stack
        if n == 0 then break end
        local fr = stack[n]
        local k, x = next(fr[1], fr[3])
        if k == nil then
          stack[n] = nil
        else
          fr[3] = k
          local place = { fr[4], k }
          local ck = shell(k, place)
          rawset(fr[2], ck, shell(x, place))
        end
      end
      return result
    end
    I.copy = copy

    -- The job runs at the next pump, as a task (so `.await` works in it);
    -- its result, or its exception, is copied back and finishes the
    -- returned task at the pump after that, never in the middle of other
    -- script code.
    local function run_job(t, f, args, n, where)
      local is_async = I.async_fns[f]
      local job = I.start(function()
        local r = f(unpack(args, 1, n))
        -- An async function's job ends when its own task does.
        if is_async then r = rt.Task.await(r) end
        return r
      end)
      rawset(job, "_observed", true)
      I.when_done(job, function(state, v)
        local ok, res = pcall(copy, v, {}, state == "done" and "result" or "exception")
        if not ok then
          state, res = "failed", I.to_exception(res)
        elseif state == "failed" and where ~= nil and I.is_exception(res) then
          -- The worker's traceback, followed by the line that started the
          -- job.
          local trace = rawget(res, "trace")
          rawset(res, "trace", (trace and (tostring(trace) .. "\n") or "") .. "job started at " .. where)
        end
        I.post(function() I.finish(t, state, res) end)
      end)
    end

    local function spawn_job(where, f, args, n, parts)
      check_callee(f, "callee")
      local seen, copied = {}, {}
      for i = 1, n do copied[i] = copy(args[i], seen, parts[i]) end
      local t = rt.newtask()
      I.post(function() run_job(t, f, copied, n, where) end)
      return t
    end

    function rt.spawn(how, x, ...)
      local f, args, n, parts = spawn_parts(how, x, ...)
      return spawn_job(caller_position(), f, args, n, parts)
    end

    -- `Worker.run(f, args...)`: `spawn` as a function.
    function Worker.run(f, ...)
      local n = select("#", ...)
      local parts = {}
      for i = 1, n do parts[i] = "argument " .. (i + 1) end
      check_callee(f, "argument 1")
      return spawn_job(caller_position(), f, { ... }, n, parts)
    end

    function Worker.map(list, f, ...)
      local where = caller_position()
      return map_with(function(g, ...)
        local n = select("#", ...)
        local parts = { "argument 2", "argument 1", "argument 1" }
        for i = 4, n do parts[i] = "argument " .. (i - 1) end
        return spawn_job(where, g, { ... }, n, parts)
      end, list, f, ...)
    end
  else
    -------------------------------------------------------------------------
    -- The worker runtime (the fork).

    local corunning = coroutine.running

    -- What this state is: nil (the main state), "pool" or "dedicated".
    local kind, count = HW.info()
    Worker.count = count
    function rt.workercount(n) Worker.count = n end

    -- True while a pool worker loads a file for a job.
    I.worker_loading = false
    local function check_not_loading(what)
      if I.worker_loading then
        I.throw("WorkerError", what .. " cannot run while a pool worker is loading a file for a job"
          .. ": move it out of the file's top level, or guard it with `unless Worker.parent`")
      end
    end

    -- The encoder's questions (lj_nltransfer.c, header).

    -- What a value with metatable `mt` is; `v` is the first such value.
    local function classify(mt, v)
      if mt == I.Range then return "builtin", "Range" end
      if mt == I.Task then return "bad", "a task" end
      local info = I.info[mt]
      if info ~= nil then
        if info.kind == "struct" then
          local nm = named[mt]
          if nm == nil then return "bad", "an instance of struct " .. tostring(info.name) .. ", " .. NO_NAME end
          return "named", nm[1], nm[2]
        elseif (info.kind == "host" or info.kind == "builtin") and type(v) == "userdata" then
          -- Only a userdata goes through a host type's transfer pair; a
          -- table with such a metatable is a table with a metatable that
          -- is not a struct, as in the stand-in.
          local tr = transfer[info.name]
          if tr ~= nil then return "host", info.name, tr[1] end
          return "bad", "a value of type " .. tostring(rt.typeof(v))
        end
      end
      local reg = reg_by_meta[mt]
      if reg ~= nil then return "meta", reg end
      if type(v) == "table" then
        -- mt is the private metatable of a struct: v is the struct itself.
        local vinfo = I.info[v]
        if vinfo ~= nil and vinfo.kind == "struct" then return "type" end
      else
        return "bad", "a value of type " .. tostring(rt.typeof(v))
      end
      return "bad", "a table with a metatable that is not a struct"
    end

    local builtin = { Range = function() return I.Range end, Task = function() return I.Task end,
      Regex = function() return I.Regex end }

    -- A type value crosses by name.
    local function typevalue(v)
      local nm = named[v]
      if nm ~= nil then return "named", nm[1], nm[2] end
      if v == I.Task then return "builtin", "Task" end
      if v == I.Regex then return "builtin", "Regex" end
      if v == I.Range then return "builtin", "Range" end
      local info = I.info[v]
      if info ~= nil and info.kind == "struct" then return "bad", "struct " .. tostring(info.name) .. ", " .. NO_NAME end
      return "bad", "the " .. tostring(info and info.kind or "type") .. " " .. tostring(info and info.name or "?")
        .. ", " .. NO_NAME
    end

    local function fail(code, value, part, keys, detail)
      local what
      if code == "function" then
        what = "a function without a name"
      elseif code == "coroutine" then
        what = "a coroutine"
      elseif code == "userdata" then
        what = "a value of type " .. tostring(rt.typeof(value))
      elseif code == "selfhost" then
        what = "a value of type " .. tostring(rt.typeof(value)) .. " inside the value its transfer function made of it"
      elseif code == "callback" then
        what = "a value of type " .. tostring(rt.typeof(value)) .. ": its encode function failed: " .. I.describe(detail)
      elseif code == "nothing" then
        what = "a value of type " .. tostring(rt.typeof(value)) .. ": its encode function returned nothing"
      else
        what = tostring(value)
      end
      local path = {}
      for i = 1, #keys do path[i] = key_text(keys[i]) end
      send_error(what, part, tconcat(path))
    end

    -- A file the message refers to and this state lacks is loaded as
    -- `import` does. A file that has registered names here has run here
    -- (or is running): the main file of a program, too.
    local function load_file(file)
      if ssub(file, 1, 1) == "=" then return end
      if by_file[file] ~= nil or I.loaded[file] ~= nil then return end
      local was = I.worker_loading
      if kind == "pool" then I.worker_loading = true end
      local ok, exports, err = pcall(I.import_name, file)
      I.worker_loading = was
      if not ok then error(exports, 0) end
      if exports == nil then error(err, 0) end
      if kind == "pool" then HW.loaded(file) end
    end

    local function receive_error(text)
      I.throw("SendError", "cannot receive " .. text)
    end

    -- A decode function of a host type that raised or did not make a
    -- userdata.
    local function recvfail(code, name, detail)
      if code == "failed" then
        receive_error("a value of type " .. tostring(name) .. ": its decode function failed: " .. I.describe(detail))
      end
      receive_error("a value of type " .. tostring(name) .. ": its decode function returned "
        .. (detail == nil and "nil" or ("a " .. tostring(rt.typeof(detail)))) .. ", not a userdata")
    end

    local function resolve(rkind, file, name)
      if rkind == 1 then
        local names = by_file[file]
        local v = names and names[name]
        if v == nil then receive_error("`" .. name .. "` of " .. file .. ": this state has no such name") end
        return v
      elseif rkind == 2 then
        local b = builtin[name]
        local v = b and b()
        if v == nil then receive_error("the built-in type " .. name) end
        return v
      elseif rkind == 3 then
        local mt = reg_by_name[name]
        if mt == nil then receive_error("a table with the metatable '" .. name .. "': this state has not registered it (Worker.register)") end
        return mt
      else
        local tr = transfer[name]
        if tr == nil then receive_error("a value of type " .. name .. ": this state has no transfer functions for it (nl_settransfer)") end
        return tr[2]
      end
    end

    HW.setup({
      named = named, other_info = I.other_info, classify = classify, typevalue = typevalue,
      fail = fail, recvfail = recvfail, load = load_file, resolve = resolve,
    })

    -- The values of a message as a table, decoded in a protected call;
    -- false and the exception when that fails.
    local function pack(...) return { n = select("#", ...), ... } end
    local function decode(box)
      return xpcall(function() return pack(HW.decode(box)) end, rt.handler)
    end

    -- Requests sent from this state: id -> { task, where }.
    local next_id = 0
    local waiting = {}

    local function add_trace(e, where)
      if where ~= nil and I.is_exception(e) then
        local trace = rawget(e, "trace")
        rawset(e, "trace", (trace and (tostring(trace) .. "\n") or "") .. "job started at " .. where)
      end
    end

    -- The state of the job each coroutine works for: the job's own task,
    -- every task started while one of its coroutines runs (an async call, a
    -- spawn's waiter, `Task.all`'s items; I.on_start), and every coroutine
    -- made then (coroutine.create and coroutine.wrap, below), so that
    -- Worker.parent names that job's state in all of them.
    local job_parent = setmetatable({}, { __mode = "k" })
    -- The job a message being handled belongs to, outside its tasks: a job
    -- while its arguments are decoded (its files load), and the job that
    -- made a spawn or a call while the result is decoded and delivered
    -- (rt.workerin), so that the callbacks its task runs (on_done,
    -- on_error) belong to that job.
    local delivering_parent = nil
    -- The job of the running code; nil for code that runs for no job.
    local function current_job()
      local co = corunning()
      return (co ~= nil and job_parent[co]) or delivering_parent
    end
    I.on_start = function(co, creator)
      local p = (creator ~= nil and job_parent[creator]) or delivering_parent
      if p ~= nil then job_parent[co] = p end
    end
    if kind ~= nil and type(coroutine) == "table" then
      -- In a worker, coroutines made by a job inherit the job's state.
      local create, cowrap = coroutine.create, coroutine.wrap
      local resume = coroutine.resume
      local function inherit(co)
        local p = current_job()
        if p ~= nil then job_parent[co] = p end
        return co
      end
      coroutine.create = function(f) return inherit(create(f)) end
      if type(cowrap) == "function" then
        -- As coroutine.wrap: the values, or the error raised again.
        local function pass(ok, ...)
          if not ok then error((...), 0) end
          return ...
        end
        coroutine.wrap = function(f)
          local co = inherit(create(f))
          return function(...) return pass(resume(co, ...)) end
        end
      end
    end

    -- A job for the pool. The task is made once the job is queued:
    -- SendError comes first, before anything is queued.
    local function spawn_job(where, f, args, n, parts)
      check_not_loading("A spawn")
      check_callee(f, "callee")
      local labels = { "callee" }
      for i = 1, n do labels[i + 1] = parts[i] end
      next_id = next_id + 1
      local id = next_id
      local ok, err = HW.spawn(labels, id, named[f][1], f, unpack(args, 1, n))
      if not ok then I.throw("WorkerError", err) end
      local t = rt.newtask()
      waiting[id] = { t, where, current_job() }
      return t
    end

    function rt.spawn(how, x, ...)
      local f, args, n, parts = spawn_parts(how, x, ...)
      return spawn_job(caller_position(), f, args, n, parts)
    end

    function Worker.run(f, ...)
      local n = select("#", ...)
      local parts = {}
      for i = 1, n do parts[i] = "argument " .. (i + 1) end
      check_callee(f, "argument 1")
      return spawn_job(caller_position(), f, { ... }, n, parts)
    end

    function Worker.map(list, f, ...)
      local where = caller_position()
      return map_with(function(g, ...)
        local n = select("#", ...)
        local parts = { "argument 2", "argument 1", "argument 1" }
        for i = 4, n do parts[i] = "argument " .. (i - 1) end
        return spawn_job(where, g, { ... }, n, parts)
      end, list, f, ...)
    end

    -- Answers request `req` (a job or a call) with `v`. A value that cannot
    -- cross fails the task with the SendError instead.
    local function reply(req, state, v)
      local ok, err = pcall(HW.reply, req, state ~= "done", v)
      if ok then return end
      local e = I.to_exception(err)
      if not pcall(HW.reply, req, true, e) then
        pcall(HW.replyerror, req, "the result could not be sent: " .. I.describe(e))
      end
    end

    -- Runs f(args) as a task and answers req when it ends; an async
    -- function's job ends when its own task does.

    local function run(req, port, f, args, first)
      local is_async = I.async_fns[f]
      local job = I.start(function()
        local co = corunning()
        if co ~= nil then job_parent[co] = port end
        local r = f(unpack(args, first, args.n))
        if is_async then r = rt.Task.await(r) end
        return r
      end)
      rawset(job, "_observed", true)
      I.when_done(job, function(state, v) reply(req, state, v) end)
    end

    -- The state of a worker (set by rt.workerstart).
    local pool_failure = nil       -- pool: why every job fails (the init function)
    local load_failure = nil       -- dedicated: why every call fails
    local exports = nil            -- dedicated: the file's export table
    local parent_port = nil        -- dedicated: the state that started it

    -- A job of the pool (lj_nlworker.c, job_f).
    function rt.workerjob(box, req, port)
      if pool_failure ~= nil then
        HW.replyerror(req, pool_failure)
        return
      end
      -- Worker.parent is the job's state while its files load, too: a top
      -- level guarded by `unless Worker.parent` stays quiet.
      local was = delivering_parent
      delivering_parent = port
      local ok, vals = decode(box)
      delivering_parent = was
      if not ok then
        reply(req, "failed", vals)
        return
      end
      run(req, port, vals[1], vals, 2)
    end

    -- Timers for Task.sleep in a worker: no host timer.
    local timers, timer_seq = {}, 0
    local function worker_timer(seconds, wake)
      if not (seconds > 0) then seconds = 0 end
      timer_seq = timer_seq + 1
      timers[#timers + 1] = { HW.now() + seconds, timer_seq, wake }
    end

    -- Fires the timers that are due, in order; returns the next deadline
    -- (or nil) and whether it did anything or the runtime has queued work.
    function rt.workerstep()
      local fired = false
      if #timers > 0 then
        local now = HW.now()
        local due, keep = {}, {}
        for i = 1, #timers do
          local t = timers[i]
          if t[1] <= now then due[#due + 1] = t else keep[#keep + 1] = t end
        end
        if #due > 0 then
          timers = keep
          tsort(due, function(a, b)
            if a[1] ~= b[1] then return a[1] < b[1] end
            return a[2] < b[2]
          end)
          fired = true
          for i = 1, #due do
            local ok, e = xpcall(due[i][3], rt.handler)
            if not ok then rt.unhandled(e) end
          end
        end
      end
      local nearest = nil
      for i = 1, #timers do
        local d = timers[i][1]
        if nearest == nil or d < nearest then nearest = d end
      end
      return nearest, fired or #I.queue > 0
    end

    -- A new worker state, after the host's init function (`failure`: its
    -- error). A dedicated worker loads its file here: its top level is its
    -- startup code.
    function rt.workerstart(wkind, from, path, failure)
      kind = wkind
      Worker.count = select(2, HW.info())
      rt.settimer(worker_timer)
      if wkind == "pool" then
        pool_failure = failure
        return
      end
      parent_port = HW.parent()
      if failure ~= nil then
        load_failure = rt.new(rt.WorkerError, { message = failure })
        return
      end
      local name = host.resolve(from, path)
      if name == nil then
        load_failure = rt.handler("cannot find the file '" .. tostring(path) .. "' to start a worker")
        return
      end
      local ok, ex, err = xpcall(I.import_name, rt.handler, name)
      if not ok then
        load_failure = ex
      elseif ex == nil then
        load_failure = rt.handler(err)
      elseif rawget(ex, "emit") ~= nil or rawget(ex, "stop") ~= nil then
        local which = rawget(ex, "emit") ~= nil and "emit" or "stop"
        load_failure = rt.new(rt.WorkerError, { message = "the worker file " .. name .. " exports `" .. which
          .. "`: a worker file may not export a function named emit or stop" })
      else
        exports = ex
      end
    end

    -- A message from the inbox (lj_nlworker.c, env_deliver).
    function rt.workerin(what, a, b, c, d)
      if what == "result" then
        -- a: id, b: status (0 done, 1 failed, 2 WorkerError), c: box, d: text
        local w = waiting[a]
        if w == nil then return end
        waiting[a] = nil
        -- The result is decoded (files it needs load) and the task's
        -- callbacks run inside I.finish for the job that made the request.
        local was = delivering_parent
        delivering_parent = w[3]
        local state, value
        if b == 2 then
          state, value = "failed", rt.new(rt.WorkerError, { message = tostring(d) })
        else
          local ok, vals = decode(c)
          if ok then
            state, value = (b == 0) and "done" or "failed", vals[1]
          else
            state, value = "failed", vals
          end
        end
        if state == "failed" then add_trace(value, w[2]) end
        local ok, err = pcall(I.finish, w[1], state, value)
        delivering_parent = was
        if not ok then error(err, 0) end
      elseif what == "call" then
        -- a: request, b: box, c: the caller's port
        if load_failure ~= nil then
          reply(a, "failed", load_failure)
          return
        end
        local ok, vals = decode(b)
        if not ok then
          reply(a, "failed", vals)
          return
        end
        local name = vals[1]
        local f = exports ~= nil and rawget(exports, name) or nil
        if type(f) ~= "function" then
          -- As a call of a member a module does not have: the value is nil,
          -- and calling it is the VM's error, a LuaError.
          reply(a, "failed", rt.new(rt.LuaError, {
            message = "attempt to call a nil value (the worker file has no pub fn named '" .. tostring(name) .. "')",
          }))
          return
        end
        run(a, parent_port, f, vals, 2)
      elseif what == "emit" then
        -- a: box. Event.run(event, args...) as a task, so that a handler
        -- may await; a failure nothing handles goes to the unhandled hook.
        local ok, vals = decode(a)
        if not ok then
          rt.unhandled(vals)
          return
        end
        I.start(function() rt.Event.run(unpack(vals, 1, vals.n)) end)
      end
    end

    ---------------------------------------------------------------------------
    -- Dedicated workers.

    local Handle = {}
    I.typeof_special[Handle] = "Worker"
    local callers = {}

    local function handle_of(h)
      local w = type(h) == "table" and rawget(h, "_w") or nil
      if w == nil or getmetatable(h) ~= Handle then rt.typeerror("the receiver", "Worker", h) end
      return w
    end

    local function call(h, name, ...)
      local w = handle_of(h)
      local n = select("#", ...)
      local labels = { "callee" }
      for i = 1, n do labels[i + 1] = "argument " .. i end
      next_id = next_id + 1
      local id = next_id
      HW.call(w, labels, id, name, ...)
      local t = rt.newtask()
      waiting[id] = { t, caller_position(), current_job() }
      return t
    end

    local function emit_labels(n)
      local labels = {}
      for i = 1, n do labels[i] = "argument " .. i end
      return labels
    end

    -- `worker.emit(event, args...)`: Event.run in the worker; nothing
    -- comes back.
    local handle_emit = I.mark(function(h, ...)
      HW.emit(handle_of(h), emit_labels(select("#", ...)), ...)
    end)
    -- `worker.stop`: lets the message being handled finish, then ends the
    -- worker; calls still waiting fail with WorkerError.
    local handle_stop = I.mark(function(h)
      HW.stop(handle_of(h))
    end)

    -- `worker.name(args)`: calls the `pub fn` of that name in the worker.
    Handle.__index = function(_, name)
      if name == "emit" then return handle_emit end
      if name == "stop" then return handle_stop end
      if type(name) ~= "string" then return nil end
      local f = callers[name]
      if f == nil then
        f = I.mark(function(h, ...) return call(h, name, ...) end)
        callers[name] = f
      end
      return f
    end
    Handle.__tostring = function() return "Worker" end

    -- `Worker.new(path)`: the path resolves as in `import`, from the file
    -- that calls it.
    function Worker.new(path)
      check_not_loading("Worker.new")
      if type(path) ~= "string" then rt.typeerror("argument 1 of Worker.new", rt.T.String, path) end
      local w, err = HW.new(caller_file(), path)
      if w == nil then I.throw("WorkerError", err) end
      return setmetatable({ _w = w }, Handle)
    end

    -- `Worker.parent`: in a worker, a handle with `emit` for the state that
    -- started the worker, or the current job. In a pool worker that is the
    -- job whose task is running: the job's own task, or a task started
    -- while one of the job's tasks ran (I.on_start). Code that runs for no
    -- job (a handler of an event, a timer's callback outside a task) gets
    -- nil, as the main state does; a file that loads for no job raises
    -- WorkerError instead, so that a top level guarded by
    -- `unless Worker.parent` never takes the worker for the main state.
    local Parent = {}
    I.typeof_special[Parent] = "Worker"
    local parent_emit = I.mark(function(h, ...)
      HW.emit(rawget(h, "_port"), emit_labels(select("#", ...)), ...)
    end)
    Parent.__index = function(_, name)
      if name == "emit" then return parent_emit end
      return nil
    end
    local parents = setmetatable({}, { __mode = "k" })
    local function parent_handle(port)
      local h = parents[port]
      if h == nil then
        h = setmetatable({ _port = port }, Parent)
        parents[port] = h
      end
      return h
    end

    setmetatable(Worker, {
      __index = function(_, k)
        if k ~= "parent" or kind == nil then return nil end
        local port
        if kind == "pool" then
          port = current_job()
          if port == nil and I.worker_loading then
            I.throw("WorkerError", "Worker.parent is unknown here: this pool worker is loading a file"
              .. " for code that runs for no job")
          end
        else
          port = parent_port
        end
        if port == nil then return nil end
        -- A job that this worker started for itself (a nested spawn run on
        -- the same pool worker) names this worker: an event sent there
        -- would vanish, so it is an error instead.
        if HW.isself(port) then
          I.throw("WorkerError", "Worker.parent names this worker itself here: the code runs for no job"
            .. " of another state")
        end
        return parent_handle(port)
      end,
    })
  end
end

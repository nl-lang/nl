-- Tasks and the pump.
--
-- Fixed names: rt.newtask, rt.resolve, rt.reject, rt.taskstate,
-- rt.settimer, rt.setunhandled, rt.unhandled, rt.pending. Additions:
-- rt.pump, rt.pumpall, rt.post. Intrinsic: rt.async. The library table: rt.Task (`Task`).
--
-- A task is a table whose metatable is `Task`; its state is kept in
-- fields with a leading underscore:
--
--   _state     "pending", "done" or "failed"
--   _value     the value (done) or the exception (failed)
--   _waiters   functions w(state, value) run when the task finishes
--   _observed  true once something awaits the task or handles its error
--
-- The body of an `async` function runs in a coroutine of its own. `.await`
-- in that coroutine (at any call depth) registers a waiter on the awaited
-- task and yields; finishing that task runs its waiters at once, which
-- resumes the coroutine (there is no scheduler).
--
-- Internal (I): I.Task, I.istask, I.finish, I.when_done, I.start (runs a
-- function as a new task), I.async_fns (the functions rt.async made),
-- I.post (queues a function for the next pump).
do
  local Task = {}
  Task.__index = Task
  rt.Task = Task
  I.Task = Task
  I.typeof_special[Task] = "Task"
  -- A built-in type, not a struct: `x is Task` holds for
  -- tasks, and rt.new cannot make one.
  I.info[Task] = { kind = "builtin", name = "Task" }

  local function pack(...) return { n = select("#", ...), ... } end

  -- Work queued for the next pump: functions run in order.
  local queue = {}
  I.queue = queue
  function I.post(f)
    queue[#queue + 1] = f
  end

  -- Addition: queues `f()` for the next rt.pump (the Lua side of nl_post:
  -- the callback runs at the next pump, protected).
  function rt.post(f)
    if type(f) ~= "function" then error("post: expects a function, not " .. rt.typeof(f), 2) end
    I.post(f)
    return true
  end

  -- Tasks that have not finished.
  local pending = 0

  function rt.pending()
    return pending
  end

  -- Unhandled failures. The default hook prints the exception
  -- and its trace to stderr.
  local function write_stderr(text)
    if stderr_write ~= nil then stderr_write(stderr, text) end
  end
  local function default_hook(e)
    write_stderr("unhandled task failure: " .. I.describe(e) .. "\n")
    if type(e) == "table" and rawget(e, "trace") then write_stderr(tostring(rawget(e, "trace")) .. "\n") end
  end
  local unhandled_hook = default_hook
  -- `nil` restores the default hook.
  function rt.setunhandled(f) unhandled_hook = f or default_hook end
  -- Reports `e` through the hook. A hook that fails is reported on stderr
  -- and never raises into the caller.
  function rt.unhandled(e)
    local ok, err = pcall(unhandled_hook, e)
    if not ok then
      pcall(write_stderr, "error in the unhandled-failure hook: " .. I.describe(err) .. "\n")
    end
  end

  -- Runs what is queued; false when there was nothing to run. Raises
  -- nothing: an error in a queued function goes to the unhandled hook.
  -- What the queued functions queue in turn waits for the
  -- next call.
  function rt.pump()
    if #queue == 0 then return false end
    local items = {}
    for i = 1, #queue do items[i] = queue[i]; queue[i] = nil end
    for i = 1, #items do
      local ok, e = xpcall(items[i], rt.handler)
      if not ok then rt.unhandled(e) end
    end
    return true
  end

  -- Addition: one pump of everything this state is sent, for a host that
  -- pumps from Lua (runtime/gmod/host.lua). On the fork it is the host's
  -- host.pump, nl_pump: the callbacks posted to the state (the results and
  -- events of workers arrive that way), a step of the workers that have no
  -- thread, then rt.pump. Elsewhere it is rt.pump.
  local host_pump = type(host) == "table" and host.pump or nil
  if type(host_pump) == "function" then
    function rt.pumpall() return host_pump() == true end
  else
    rt.pumpall = rt.pump
  end

  local function istask(t)
    return type(t) == "table" and getmetatable(t) == Task
  end
  I.istask = istask

  local function checktask(t, what)
    if not istask(t) then rt.typeerror(what, Task, t) end
  end

  -- `reject`'s conversion: that of `raise` (a string becomes an Exception),
  -- and any other value that is not an exception is wrapped as `try` would
  -- wrap it (LuaError, after the host's translator).
  local function to_exception(e)
    if type(e) == "string" then return rt.new(rt.Exception, { message = e }) end
    if I.is_exception(e) then return e end
    return rt.handler(e)
  end
  I.to_exception = to_exception

  function rt.newtask()
    pending = pending + 1
    return setmetatable({ _state = "pending" }, Task)
  end
  local newtask = rt.newtask

  -- A waiter or callback runs protected: an error in one is reported
  -- through the unhandled hook, and the other waiters still run.
  local function run_waiter(w, state, value)
    local ok, e = xpcall(w, rt.handler, state, value)
    if not ok then rt.unhandled(e) end
  end

  -- Waiters to run: a stack of frames { list, next index, count, state,
  -- value }, one per finished task whose waiters have not all run. The
  -- loop in `finish` takes the next waiter of the top frame, so the order
  -- is the one a recursive walk would give (a task finished by a waiter
  -- has its own waiters run before the next waiter of the first task),
  -- without the stack growing with the length of a chain of tasks that
  -- await each other.
  local frames = {}
  -- Whose call of `finish` runs the loop: the coroutine, or MAIN for the
  -- main thread; nil when no loop runs.
  local MAIN = {}
  local draining = nil

  local function running_key()
    local co, ismain = corunning()
    if co == nil or ismain then return MAIN end
    return co
  end

  -- True when a loop is running below the current call: its owner is the
  -- main thread, the running coroutine, or one that resumed it. A loop
  -- whose coroutine is suspended (a callback in it yielded) does not
  -- count: the frames it left are run by the next loop, and by that one
  -- too if it is resumed, each waiter once.
  local function drain_active()
    if draining == nil then return false end
    if draining == MAIN then return true end
    local st = costatus(draining)
    return st == "running" or st == "normal"
  end

  -- Finishes `t`. Finishing a task twice does nothing the second time.
  -- The waiters (awaiting tasks, `on_done` and
  -- `on_error` callbacks) run in the order they were added, and they have
  -- all run when the outermost `finish` returns: a task finished while
  -- waiters of another one run (a resumed task that resolves another, a
  -- `Task.all`) has its waiters run by that outer call, after the waiter
  -- that finished it has returned, not inside it.
  local function finish(t, state, value)
    if rawget(t, "_state") ~= "pending" then return end
    rawset(t, "_state", state)
    rawset(t, "_value", value)
    pending = pending - 1
    local ws = rawget(t, "_waiters")
    rawset(t, "_waiters", nil)
    if state == "failed" and not rawget(t, "_observed") then
      -- A failure that nothing awaits or handles goes to the
      -- host hook. Whether something does is decided at the next pump, so
      -- that a task awaited (or given `on_error`) after it failed, in the
      -- same run of script code, is not reported.
      I.post(function()
        if not rawget(t, "_observed") then rt.unhandled(value) end
      end)
    end
    if ws == nil or #ws == 0 then return end
    frames[#frames + 1] = { ws, 1, #ws, state, value }
    if drain_active() then return end
    local me = running_key()
    draining = me
    while true do
      local n = #frames
      if n == 0 then break end
      local fr = frames[n]
      local i = fr[2]
      local w = fr[1][i]
      fr[1][i] = nil
      -- A frame is dropped as its last waiter starts, so that a chain
      -- leaves no frames behind.
      if i >= fr[3] then frames[n] = nil else fr[2] = i + 1 end
      run_waiter(w, fr[4], fr[5])
    end
    if draining == me then draining = nil end
  end
  I.finish = finish

  function rt.resolve(t, v)
    checktask(t, "argument 1 of resolve")
    finish(t, "done", v)
  end

  function rt.reject(t, e)
    checktask(t, "argument 1 of reject")
    finish(t, "failed", to_exception(e))
  end

  function rt.taskstate(t)
    checktask(t, "argument 1 of taskstate")
    local st = rawget(t, "_state")
    if st == "pending" then return "pending" end
    return st, rawget(t, "_value")
  end

  -- Runs `w(state, value)` when `t` finishes, or at once when it has.
  local function when_done(t, w)
    local st = rawget(t, "_state")
    if st ~= "pending" then
      run_waiter(w, st, rawget(t, "_value"))
      return
    end
    local ws = rawget(t, "_waiters")
    if ws == nil then
      ws = {}
      rawset(t, "_waiters", ws)
    end
    ws[#ws + 1] = w
  end
  I.when_done = when_done

  -- Timer: f(seconds, wake); wake() ends the sleep.
  local timer = nil
  function rt.settimer(f) timer = f end
  I.timer = function() return timer end

  ---------------------------------------------------------------------------
  -- Coroutines.

  -- What `.await` yields; any other value yielded by a task's coroutine is
  -- a yield that did not come from `.await`.
  local AWAIT = {}

  -- The task each running task coroutine belongs to.
  local task_of = setmetatable({}, { __mode = "k" })

  local function body(f, ...)
    return xpcall(f, rt.handler, ...)
  end

  -- Resumes a task's coroutine and, when it has ended, finishes its task.
  local function step(co, ...)
    if costatus(co) ~= "suspended" then
      -- Cannot happen: a task waits on one task at a time and is resumed
      -- once. Reported rather than raised into whoever finished the task.
      rt.unhandled(rt.new(rt.Exception, { message = "NL runtime: a task was resumed while it was not waiting" }))
      return
    end
    local r = pack(coresume(co, ...))
    if costatus(co) == "dead" then
      local t = task_of[co]
      task_of[co] = nil
      if t == nil then return end
      if not r[1] then
        finish(t, "failed", to_exception(r[2]))
      elseif r[2] then
        finish(t, "done", r[3])
      else
        finish(t, "failed", r[3])
      end
    elseif r[2] ~= AWAIT then
      -- The coroutine was suspended by something other than `.await`
      -- (`Coroutine.yield` called in a task): nothing would resume it.
      local t = task_of[co]
      task_of[co] = nil
      if t ~= nil then
        finish(t, "failed", rt.new(rt.Exception, { message = "a task cannot yield except through .await" }))
      end
    end
  end

  -- Runs f(...) as a new task: at once, in a coroutine of its own, until
  -- it ends or awaits. Returns the task.
  -- I.on_start(new, creator), when set, learns of each task coroutine and
  -- the coroutine that started it (nl_64_worker.lua: Worker.parent).
  function I.start(f, ...)
    local t = newtask()
    local co = cocreate(body)
    task_of[co] = t
    local hook = I.on_start
    if hook ~= nil then hook(co, (corunning())) end
    step(co, f, ...)
    return t
  end

  -- `async fn` and `async do`: calling the result
  -- runs `f` as a new task and returns the task. The task's value is the
  -- first value `f` returns.
  local async_fns = setmetatable({}, { __mode = "k" })
  I.async_fns = async_fns
  function rt.async(f)
    local start = I.start
    local w = function(...) return start(f, ...) end
    async_fns[w] = true
    return w
  end

  -- The running task coroutine, or nil outside a task.
  local function current()
    local co, ismain = corunning()
    if co == nil or ismain then return nil end
    if task_of[co] == nil then return nil end
    return co
  end

  ---------------------------------------------------------------------------
  -- The `Task` library.

  -- `task.await`: the value, or the exception raised again.
  -- Outside a task it raises, whether or not the task has
  -- finished.
  function Task.await(t)
    checktask(t, "the receiver of .await")
    local co = current()
    if co == nil then
      -- The error has no type of its own; it is a plain Exception.
      I.throw("Exception", ".await outside a task: only code running in an async function or block can wait")
    end
    local observed = rawget(t, "_observed")
    rawset(t, "_observed", true)
    if rawget(t, "_state") == "pending" then
      local w = function() step(co) end
      when_done(t, w)
      local ok, err = pcall(coyield, AWAIT)
      if not ok then
        -- The yield failed (across a C call, for instance): nothing is
        -- waiting, so the waiter is withdrawn, and the task is as observed
        -- as it was, before the error goes on. A failure of the task is
        -- then still reported when nothing else handles it.
        local ws = rawget(t, "_waiters")
        if ws ~= nil then
          for i = #ws, 1, -1 do
            if ws[i] == w then tremove(ws, i) break end
          end
        end
        rawset(t, "_observed", observed)
        error(err, 0)
      end
    end
    if rawget(t, "_state") == "done" then return rawget(t, "_value") end
    error(rawget(t, "_value"), 0)
  end

  -- `task.on_done(f)`: f(value) once the task has finished with a value;
  -- at once when it already has. Returns nothing.
  function Task.on_done(t, f)
    checktask(t, "the receiver of on_done")
    if type(f) ~= "function" then rt.typeerror("argument 1 of on_done", rt.T.Function, f) end
    when_done(t, function(state, v)
      if state == "done" then f(v) end
    end)
  end

  -- `task.on_error(f)`: f(exception) once the task has failed. The failure
  -- counts as handled.
  function Task.on_error(t, f)
    checktask(t, "the receiver of on_error")
    if type(f) ~= "function" then rt.typeerror("argument 1 of on_error", rt.T.Function, f) end
    rawset(t, "_observed", true)
    when_done(t, function(state, e)
      if state == "failed" then f(e) end
    end)
  end

  rt.method(Task.await)
  rt.method(Task.on_done)
  rt.method(Task.on_error)

  -- `Task.new(fn(resolve, reject))`: the function runs at
  -- once; `resolve(v)` and `reject(e)` finish the task, and later calls do
  -- nothing; an error raised by the function fails the task, as `reject`
  -- would, and one raised after it finished the task is dropped.
  function Task.new(f)
    if type(f) ~= "function" then rt.typeerror("argument 1 of Task.new", rt.T.Function, f) end
    local t = newtask()
    local function resolve(v) finish(t, "done", v) end
    local function reject(e) finish(t, "failed", to_exception(e)) end
    local ok, e = xpcall(f, rt.handler, resolve, reject)
    if not ok then finish(t, "failed", e) end
    return t
  end

  local function checklist(list, what)
    if type(list) ~= "table" then rt.typeerror("argument 1 of " .. what, rt.T.Table, list) end
    local n = #list
    for i = 1, n do
      if not istask(list[i]) then rt.typeerror("item " .. i .. " of the list given to " .. what, Task, list[i]) end
    end
    return n
  end

  -- `Task.all(list)`: a task of the list of values, in list order; it fails
  -- with the first failure. Its items count as awaited.
  function Task.all(list)
    local n = checklist(list, "Task.all")
    local out = newtask()
    local values = {}
    if n == 0 then
      finish(out, "done", values)
      return out
    end
    local left = n
    for i = 1, n do
      local t = list[i]
      rawset(t, "_observed", true)
      when_done(t, function(state, v)
        if state == "done" then
          values[i] = v
          left = left - 1
          if left == 0 then finish(out, "done", values) end
        else
          finish(out, "failed", v)
        end
      end)
    end
    return out
  end

  -- `Task.race(list)`: finishes as the first item to finish, with its value
  -- or its exception. Its items count as awaited. `Task.race({})` would never
  -- finish, so it raises.
  function Task.race(list)
    local n = checklist(list, "Task.race")
    if n == 0 then
      I.throw("Exception", "Task.race needs at least one task: a race of none would never finish")
    end
    local out = newtask()
    for i = 1, n do
      local t = list[i]
      rawset(t, "_observed", true)
      when_done(t, function(state, v) finish(out, state, v) end)
    end
    return out
  end

  -- `Task.sleep(seconds)`: needs the host's timer.
  function Task.sleep(seconds)
    if type(seconds) ~= "number" then rt.typeerror("argument 1 of Task.sleep", rt.T.Number, seconds) end
    if timer == nil then
      I.throw("Exception", "Task.sleep needs a timer, and the host has installed none (nl_settimer)")
    end
    local t = newtask()
    timer(seconds, function() finish(t, "done", nil) end)
    return t
  end
end

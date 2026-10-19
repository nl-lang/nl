-- Tasks (fake): resolve and reject run the waiters before they return.

local TaskT = newtype("Task", nil, "builtin")
local TASK = {}
TASK.__index = TASK
local pending = 0
local unhandled_hook = nil
local timer = nil

function TASK:on_done(f)
  if self.state == "done" then f(self.value)
  elseif self.state == "pending" then self.done[#self.done + 1] = f end
end
function TASK:on_error(f)
  self.handled = true
  if self.state == "failed" then f(self.value)
  elseif self.state == "pending" then self.err[#self.err + 1] = f end
end

function rt.newtask()
  pending = pending + 1
  return setmetatable({ state = "pending", done = {}, err = {} }, TASK)
end

function rt.resolve(task, v)
  if task.state ~= "pending" then return end
  task.state, task.value = "done", v
  pending = pending - 1
  for _, f in ipairs(task.done) do f(v) end
end

function rt.reject(task, e)
  if task.state ~= "pending" then return end
  if type(e) == "string" then e = rt.new(types.Exception, { message = e }) end
  task.state, task.value = "failed", e
  pending = pending - 1
  if #task.err == 0 and not task.handled then rt.unhandled(e) end
  for _, f in ipairs(task.err) do f(e) end
end

function rt.taskstate(task)
  if getmetatable(task) ~= TASK then error("not a task", 2) end
  if task.state == "pending" then return "pending" end
  return task.state, task.value
end

function rt.settimer(f) timer = f end
function rt.sleep(seconds)
  if not timer then rt.raise("Task.sleep needs a timer (nl_settimer)") end
  local t = rt.newtask()
  timer(seconds, function() rt.resolve(t, seconds) end)
  return t
end

function rt.setunhandled(f) unhandled_hook = f end
function rt.unhandled(e)
  if unhandled_hook then unhandled_hook(e)
  else io.stderr:write("unhandled: ", rt.tostring(e), "\n") end
end
function rt.pending() return pending end

-- The runtime's own queue (runtime/nl.lua, ADDITIONS): rt.post(f) queues
-- f() for the next rt.pump, which nl_pump drives.
local queue = {}
function rt.post(f) queue[#queue + 1] = f; return true end
function rt.pump()
  if #queue == 0 then return false end
  local items = queue
  queue = {}
  for i = 1, #items do
    local ok, e = xpcall(items[i], rt.handler)
    if not ok then rt.unhandled(e) end
  end
  return true
end

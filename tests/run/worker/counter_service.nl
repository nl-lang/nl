# Helper for worker_dedicated.nl: a dedicated worker with state of its own.
let total = 0

pub fn add(n)
  total += n
  total
end

pub fn get_total
  total
end

on Ping do |x|
  Worker.parent.emit "Pong", x * 2
end

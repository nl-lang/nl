# Helper for worker_register.nl: registers a Lua-style metatable in every state that loads it.
pub const point_meta = {}
Worker.register("Point", point_meta)

pub fn read_point(p)
  "#{getmetatable(p) == point_meta} #{p.x}"
end

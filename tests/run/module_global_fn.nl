# global fn, global module and global struct are exported and stored in the Lua global of their name
global fn class_(name: String): String
  "class #{name}"
end

global module Registry
  const items = ["a", "b"]

  fn size: Number
    Self.items.len
  end
end

global struct Vec2
  x: Number
  y: Number
end

fn Vec2.new(x: Number, y: Number): Vec2
  Vec2 { x: x, y: y }
end

fn Vec2.sum: Number
  @x + @y
end

# NL code uses them as it uses any item of the file.
print class_("Box")
print Registry.size
print Vec2.new(3, 4).sum

# Lua code finds them as globals, from the time each one is defined.
include("include/global_items.lua")
print :class_ == class_!, :Registry == Registry, :Vec2 == Vec2

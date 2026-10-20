# Lua things for the conformance programs that extend declared modules and
# types (lua_extend_declared.nl): the runner passes this directory to every
# program with `--decl decl`. include/gadgets.lua defines them.

## A Lua library table.
declare module Shelf = :shelf
  fn count(): Number
end

## A Lua class: its instances are tables whose metatable is `Gadget`,
## which is its own `__index`.
declare struct Gadget = :Gadget
  name: String
  fn label(self): String as 'Label'
end

declare fn make_gadget(name: String): Gadget = :make_gadget

## A host type that no host registers here.
declare type Widget
  fn id(self): Number as 'Id'
end

## A type nested in a module: it has no name the host registers and no
## Lua path of its own.
declare module Nest = :nest
  type Inner
  end
end

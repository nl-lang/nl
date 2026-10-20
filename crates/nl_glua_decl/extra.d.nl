## gmod's class `Vector`: a position or direction, with Lua's arithmetic.
## https://wiki.facepunch.com/gmod/Vector
declare type Vector
  x: Number
  y: Number
  z: Number
  fn +(right: Vector): Vector
  fn -(right: Vector): Vector
  fn *(right: Number | Vector): Vector
  fn /(right: Number | Vector): Vector
  fn -: Vector
end

## gmod's class `Angle`: pitch, yaw and roll in degrees, with Lua's
## arithmetic. https://wiki.facepunch.com/gmod/Angle
declare type Angle
  p: Number
  y: Number
  r: Number
  pitch: Number
  yaw: Number
  roll: Number
  fn +(right: Angle): Angle
  fn -(right: Angle): Angle
  fn *(right: Number): Angle
  fn /(right: Number): Angle
  fn -: Angle
end

## gmod's class `Color`, a Lua class over tables: `Color(255, 0, 0)` gives
## it its metatable, which `FindMetaTable("Color")` returns.
## https://wiki.facepunch.com/gmod/Color
declare struct Color = :FindMetaTable('Color')
  r: Number
  g: Number
  b: Number
  a: Number
end

# Lua's functions whose wiki types are narrower than what Lua takes: the
# replacement of gsub is a string (or number), a table or a function, and
# find gives nil when nothing matches, then the captures.
## gmod's library `string`. https://wiki.facepunch.com/gmod/string
declare module StringLib = :string
  ## Realm: shared and menu. https://wiki.facepunch.com/gmod/string.gsub
  fn gsub(string: String, pattern: String, replacement: String | Number | Table | Function, max_replaces: Number?): (String, Number)
  ## Realm: shared and menu. https://wiki.facepunch.com/gmod/string.find
  fn find(haystack: String, needle: String, start_pos: Number?, no_patterns: Bool?): (Number?, Number?, any)
end

## The realm a file runs in. Realm: shared.
declare const SERVER: Bool
## The realm a file runs in. Realm: shared.
declare const CLIENT: Bool
## True in the menu state. Realm: menu.
declare const MENU_DLL: Bool
## The invalid entity: test entities with `is_valid`, not with `nil`.
declare const NULL: Entity
## The running gamemode, once it has loaded. Realm: shared.
declare const GAMEMODE: GM
## Realm: shared.
declare const vector_origin: Vector
## Realm: shared.
declare const vector_up: Vector
## Realm: shared.
declare const angle_zero: Angle
## Realm: shared.
declare const color_white: Color
## Realm: shared.
declare const color_black: Color
## Realm: shared.
declare const color_transparent: Color

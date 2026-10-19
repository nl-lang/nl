# `self` only first, no defaults, a host type takes no lua_root, a member names its key with `as`
declare type Player = :player
  fn pos(x: Number, self): Vector
  fn name(self, greeting: String = "hi"): String
  fn hp(self): Number = :getHp
end

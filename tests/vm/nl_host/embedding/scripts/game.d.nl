## What host.c registers.
declare type Unit
  fn damage(self, amount: Number)
  fn hp(self): Number
end

declare module Engine = :engine
  fn new_unit(name: String, hp: Number): Unit
  fn log(text: String)
end

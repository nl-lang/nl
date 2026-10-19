# Declare module with a lua_root, fields with `as`, functions, consts and a nested struct
declare module Foo = :foo
  coolness_factor: Number as 'coolnessFactor'
  const limit: Number
  fn return_largest(a: Number, b: Number): Number as 'ReturnLargest'
  fn each(list: [Number], callback: Function)
  struct Point
    x: Number
    fn new(x: Number?, y: Number?): Point
    fn length(self): Number
  end
end

declare module Json = :require('json')
  fn encode(value: any): String
  fn decode(text: String): any
end

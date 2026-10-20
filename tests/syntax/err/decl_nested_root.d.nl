# A nested declaration names its Lua key with `as`, not `= :path`; parsing goes on
declare module Top = :Top
  module Sub = :sub
    fn meth(self): String
  end
  const limit: Number
end

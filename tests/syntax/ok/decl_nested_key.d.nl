# Nested modules, structs and types are members: each names its Lua key with `as`
declare module Top = :Top
  module Sub as 'SubLua'
    fn meth(self, x: String): String
  end
  struct Point as 'PointClass'
    x: Number
  end
  type Handle : Entity as 'HandleMeta'
  end
end

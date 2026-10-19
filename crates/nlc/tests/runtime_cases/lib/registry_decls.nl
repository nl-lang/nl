# Declarations whose registry names registry_names.nl checks.
struct Inner
  a: Number
end
fn f(x) x end
fn Inner.m 1 end
module M
  struct Inner
    b: Number
  end
  fn f(x) x end
  fn Inner.m 2 end
  module N
    fn f(x) x end
    struct Deep
      c: Number
    end
  end
end
module N
  fn f(x) x end
end
module M.Sub
  fn g 1 end
end
fn String.registry_shout
  self.upcase
end
interface Show
  fn show: String
end
interface Show for Inner
  fn show
    "Inner"
  end
end
if true
  struct Hidden
    d: Number
  end
  fn hidden 1 end
  fn Inner.hidden_method 1 end
end
do
  struct Hidden
    e: Number
  end
end
# `A.B.clash` would name two places: only the first is registered.
module A
  module B
    fn clash 1 end
  end
end
module A.B
  fn clash 2 end
end
# A reopened module's function, defined again: one place, the later one.
module M
  fn f(x) x + 1 end
end

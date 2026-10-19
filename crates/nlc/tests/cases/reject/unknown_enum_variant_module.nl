# An enum reached through a module of the same file has
# its variants, `values` and `name`, and nothing else.
module M
  enum Inner
    A
  end
end
print(M.Inner.A, M.Inner.values.len, M.Inner.name("A"))
# expect-error: no variant 'Nope'
print(M.Inner.Nope)

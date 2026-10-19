# A pipe target path has names only, an optional
# `:name`, then one argument list, all in the path.
a = b |> :string.upper
c = d |> :t.f(1)
e = f |> :t:m(2)

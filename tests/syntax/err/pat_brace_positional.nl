# A `{…}` pattern goes by key; a positional pattern is written with
# brackets, `[123, 456]`.
case v
when {123, 456}
  a()
when {1, {x}}
  b()
end

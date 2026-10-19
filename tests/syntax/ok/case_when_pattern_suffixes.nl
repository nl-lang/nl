# A `when_pattern` takes `as name` and `: Type`.
case v
when {a, b} as tbl: Table
  a()
when [h, *t] as all
  b()
end

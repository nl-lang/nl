# Recovery: garbage inside a `case` before the first `when` and in a `when`
# item list; the clauses parse.
case x
  print "no when yet"
when 1, , 2
  a()
when )
  b()
end
y = 1

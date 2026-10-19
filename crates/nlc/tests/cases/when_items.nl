# Finding 5: a `when` with several pattern items that bind the same names
# binds them whichever item matched; the guard sees them too.
fn f(v)
  case v
  when [x, 1], [1, x]
    "got #{x}"
  else
    "none"
  end
end
print f([5, 1])
print f([1, 7])
print f([2, 2])

fn g(v)
  case v
  when {a: n}, {b: n} if n > 10
    "big #{n}"
  when {a: n}, {b: n}
    "small #{n}"
  else
    "other"
  end
end
print g({a: 50})
print g({b: 20})
print g({b: 3})
print g({c: 1})

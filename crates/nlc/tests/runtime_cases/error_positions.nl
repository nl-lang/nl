# An error raised by an intrinsic names the NL line that applied
# it, never a line of the runtime, and a library function called as a
# member is named by the member, not by a runtime helper.
fn show(label, f)
  try
    f()
  catch e
    print label, e.message
  end
end
let n: any = nil
let num: any = 5
let s: any = "x"
let t: any = {}
show "member of nil", fn()
  let v = n.foo
  v
end
show "call member of nil", fn()
  let v = n.foo(1)
  v
end
show "missing member", fn()
  let v = t.zzz(1)
  v
end
show "len of number", fn() num.len end
show "len of nil", fn()
  n.len
end
show "idiv", fn() s // 2 end
show "band", fn()
  s & 1
end
show "bnot", fn() ~s end
show "compare", fn() t < 1 end
show "greater", fn() t > 1 end
show "string index", fn() "abc"[t] end
show "index nil", fn()
  n[1]
end
show "Table.concat", fn()
  let v = :table.concat(nil)
  v
end
show "s.rep", fn()
  let v = :String.rep(nil)
  v
end

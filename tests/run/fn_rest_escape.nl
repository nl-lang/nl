# a rest table that escapes holds only its arguments: no count among its keys, and the count still goes with it
let kept = []

fn escape(*args)
  kept.push(args)
  let keys = []
  for k, v in args
    keys.push("#{k}=#{v}")
  end
  print keys.join(" "), Table.keys(args).len, Table.size(args), args.len
end

escape(1, 2)
escape("a", nil, "c")
escape(nil, nil)
escape()

# The count goes along: a spread elsewhere keeps the trailing nils.
fn count(*args)
  :select("#", *args)
end

fn returned(*args)
  args
end

let t: any = returned(1, nil, nil)
print count(*t), [*t, 9].len, Table.unpack(t)
print t["n"] == nil, Table.keys(t).len

# Keyed arguments are taken out first; the rest are counted alone.
fn opts(*args, **o)
  kept.push(args)
  print args.len, Table.size(args), o.k, count(*args)
end
opts(1, nil, k: "v")
opts(k: "w")

# What Table.pack makes keeps its count in `n`, as Lua 5.2's does: `.n`
# is the count, `.len` is #t.
let p = Table.pack(1, nil)
print p.n, p.len, count(*p)

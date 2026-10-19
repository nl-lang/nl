# Finding 9: `{f: Function}`, `{f: f: Function}` and `[f: Function]` are the
# same binding: each name is function-kind, so a bare use calls
# it and `!` reads it.
fn hi
  "hi called"
end
let {f: Function} = {f: hi!}
print f, typeof(f!)
let {g: g: Function} = {g: hi!}
print g, typeof(g!)
let [h: Function] = [hi!]
print h, typeof(h!)

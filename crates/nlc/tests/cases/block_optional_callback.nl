# A parameter typed `Function?` takes a block, as an optional
# callback.
fn each(list: [Number], callback: Function?)
  return "no callback" if callback == nil
  for v in list
    callback(v)
  end
  "done"
end
print each([1, 2])
let r = each([1, 2]) do |v| print v end
print r

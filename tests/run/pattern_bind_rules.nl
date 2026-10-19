# Names in patterns always bind; after `key:` lowercase binds and PascalCase is a type
let me = "kim"
fn who(item)
  case item
  when {owner: me}
    "bound #{me}"
  end
end

print who({owner: "lee"})
print me

fn typed_or_bound(t)
  case t
  when {v: Number}
    "type test, bound v=#{v}"
  when {v: value}
    "bound value=#{value}"
  end
end

print typed_or_bound({v: 1})
print typed_or_bound({v: "s"})

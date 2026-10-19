# `if let pattern = expr` and `elseif let`.
if let player = find_player(name)
  greet player
elseif let {role: "admin", name} = user
  admin name
end

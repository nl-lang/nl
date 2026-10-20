# `?` after a shorthand name or a key marks an optional field, in brace
# and struct patterns: `{count?}`, `{pos?: {x, y}}`, `{name?: String}`.
case node
when DiceNode {count?, sides}
  roll(count, sides)
when {kind: "move", to?: {x, y}, speed?}
  go(x, y, speed)
end
let {name?: String, hp} = t

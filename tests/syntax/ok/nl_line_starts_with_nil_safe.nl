# `?.` and `??` at the start of a line continue the line before.
let name = player
  ?.profile
  ?.name
  ?? "nobody"
let first = list
  ?.[1]
run = handlers
  ?.done
  ?.(true)

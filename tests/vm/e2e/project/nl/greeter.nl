# Imported by main.nl and required by lua/calc.lua: it runs once per state.
let count = 0
count += 1

pub fn greet(name)
  "hello " <> name
end

pub fn runs
  count
end

# End-to-end test: an NL file that imports an NL file and
# requires a Lua module, which itself requires the same NL file.
import { greet, runs } from 'nl/greeter'

let calc = require("lua.calc")

pub fn summary
  greet("host") <> ", " <> tostring(calc.add(2, 3)) <> ", " <> calc.from_nl("lua") <> ", runs " <> tostring(runs())
end

pub fn fail
  raise "raised in main.nl"
end

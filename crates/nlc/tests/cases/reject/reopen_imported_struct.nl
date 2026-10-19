# `module X` reopens an imported X only when it is a module.
import { Box } from 'lib/kinds'
# expect-error: `Box` is an imported struct
module Box
end

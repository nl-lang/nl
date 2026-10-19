# The same for a hoisted struct.
import { Box } from 'lib/kinds'
# expect-error: `Box` is imported by this file
struct Box
  w: Number
end

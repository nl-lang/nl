# The same for a declaration that is not
# hoisted.
import { Mode } from 'lib/kinds'
print Mode.Easy
# expect-error: `Mode` is imported by this file
const Mode = 1

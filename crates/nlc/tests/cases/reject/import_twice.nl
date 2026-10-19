# A name imported twice.
import { helper } from 'lib/kinds'
# expect-error: `helper` is imported twice
import helper from 'lib/kinds'

# Rejected by the analyzer.
import { loaded } from '../import/bool_flip'

print loaded, true.flip

# expect-error: the imported file defines `Bool.flip`, and it runs first, so this one cannot define it again
fn Bool.flip
  self
end

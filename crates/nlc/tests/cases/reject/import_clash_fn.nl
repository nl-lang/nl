# A name is looked up in the file's declarations before
# an import, so a declaration with the name of an import would hide it for
# the whole file. A compile error at the declaration.
import { helper } from 'lib/kinds'
print helper(1)
# expect-error: `helper` is imported by this file
fn helper(x)
  x
end

# Directly after `import`, `from` starts the whole-file form;
# after an import name or `}` it is the keyword; elsewhere a name.
import from 'a' as A
import x from 'b'
import { from } from 'c'
spawn find_path(from, to)
fn find_path(from: Point, to: Point)
end

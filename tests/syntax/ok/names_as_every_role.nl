# `as` names the export table of `import from`, gives the handler
# name of `on`, binds the whole value in a pattern and a `when` pattern, and
# is the conversion operator elsewhere.
import from 'lib' as Lib
on E as 'h' do end
let {a} as t = v
case v
when {a} as w
  g()
end
n = v as Number

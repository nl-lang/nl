# A function of `import from … as Lib` whose parameters the compiler sees.
import from 'lib/kinds' as K
# expect-error: `K.helper` has no Function parameter
K.helper 1 do
end

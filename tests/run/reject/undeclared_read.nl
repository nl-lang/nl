# Rejected by the resolver.
let a = 1
# expect-error: `undeclared_thing` is declared nowhere (file, import, prelude, declaration file); NL has no fallback to Lua globals
print undeclared_thing

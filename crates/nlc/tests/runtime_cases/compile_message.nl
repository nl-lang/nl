# The message of a failed compile (host.compile, behind load_nl)
# says how many more errors there are.
let f, msg = load_nl("let a = = 1\nlet b = = 2\nlet c = = 3\n", "three")
print f == nil
print msg.index("more error") != nil
let g, one = load_nl("let a = = 1\n", "one")
print one.index("more error") == nil

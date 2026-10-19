# A registered name identifies one declaration in
# its file. Declarations in a module body are named by the
# path of the enclosing modules; those in a nested block are not
# registered, so they cannot be sent.
include("lib/registry_spy.lua")
include("lib/registry_decls.nl")
let names = :REGISTERED("lib/registry_decls.nl")
for name in names.split(" ")
  print name
end

fn ident(v) v end
let holder = {}
do
  struct Local
    x: Number
  end
  holder.value = Local { x: 1 }
end
try
  spawn ident(holder.value)
catch e
  print "#{typeof(e)}: #{e.message}"
end

let f, msg = load_nl("if true\n  fn helper 1 end\n  spawn helper()\nend\n", "nested")
print msg

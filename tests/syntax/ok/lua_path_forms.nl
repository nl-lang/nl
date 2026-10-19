# Lua paths: a call, command arguments, a bare path
# statement, a method call at the end, `.len` as a key, a field read after a
# call (still Lua), and an NL member after a parenthesized path.
:string.len("x")
print :string.len("some string")
let max = :config.maxPlayers
:hud.mainPanel:setVisible(true)
:http.request url, resolve, reject
:collectgarbage
n = :t.len
v = :a.b().c
w = (:a.b()).c

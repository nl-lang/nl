# Lua paths
print :string.len("some string")
print :type(5)
print :math.floor(2.7)
print :string.upper("abc")
let n = 3
print :type(n)
let cfg = {a: 1}
print :cfg.a
:_G.config = {}
:config.maxPlayers = 8
let max = :config.maxPlayers
print max
:_G.say_hi = fn print("hi from a global") end
:say_hi
:say_hi()
print typeof(:say_hi)
:_G.holder = {"len": 3}
print :holder.len
:_G.hud = {"mainPanel": {visible: false}}
:hud.mainPanel.setVisible = fn(this, v) this.visible = v end
:hud.mainPanel:setVisible(true)
print :hud.mainPanel.visible
:print "command call through a path"

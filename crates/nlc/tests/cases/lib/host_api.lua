-- The host API that lib/host_api.d.nl declares (decl_nested_module.nl,
-- decl_optional_member.nl): a module with a nested one whose Lua key differs
-- from its NL name, Lua methods that need their `self`, and a Lua class
-- whose methods have capitalized keys.
HostTop = { Sub = { tag = "sub" }, InnerLua = {} }
function HostTop.Sub:meth(x) return self.tag .. ":" .. x end
function HostTop.Sub.Plain(x) return "plain " .. x end
function HostTop.InnerLua.Hello() return "hello" end
function HostTop.InnerLua:Count() return self == HostTop.InnerLua and 1 or 0 end

PanelMeta = {}
PanelMeta.__index = PanelMeta
function PanelMeta:Remove() return "removed " .. self.name end
function PanelMeta:GetWide() return #self.name * 10 end
function FindPanel(name)
  if name == "" then return nil end
  return setmetatable({ name = name }, PanelMeta)
end
HostPackage = FindPanel("pkg")

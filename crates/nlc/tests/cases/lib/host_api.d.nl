# The declarations of lib/host_api.lua.
declare module Top = :HostTop
  module Sub
    fn meth(self, x: String): String
    fn plain(x: String): String as 'Plain'
  end
  module Inner as 'InnerLua'
    fn hello(): String as 'Hello'
    fn count(self): Number as 'Count'
  end
end

declare struct Panel = :PanelMeta
  name: String
  fn remove(self): String as 'Remove'
  fn wide(self): Number as 'GetWide'
end

declare fn find_panel(name: String): Panel? = :FindPanel
declare const PACKAGE: Panel? = :HostPackage

# Helper for import_export_kinds.nl: one export of each kind.
pub const maybe: Function? = fn() "maybe called" end
pub const always = fn() "always called" end
pub const typed: Function = fn() "typed called" end
pub fn named
  "named called"
end
pub type Num = Number
pub module Tools
  fn hi
    "hi"
  end
end
pub struct Pt
  x: Number
end
# In its own file `maybe` is a variable (`Function?` leaves the
# name a variable), so a bare `maybe` is read, not called.
pub const own_view = typeof(maybe)

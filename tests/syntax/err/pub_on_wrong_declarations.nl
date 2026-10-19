# `pub` before `let`, `global`, `fn Type.name`, an
# implementation or `on` is an error; `pub const` takes names only.
pub let a = 1
pub global B = 2
pub fn Foo.bar end
pub interface I for T end
pub on E do end
pub const {x, y} = t

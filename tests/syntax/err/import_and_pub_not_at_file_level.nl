# `import` and `pub` only at file level.
fn f
  import a from 'lib'
  pub fn g end
end
module M
  pub const x = 1
end
